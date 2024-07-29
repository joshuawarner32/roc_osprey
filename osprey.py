import requests
import hashlib
import sqlite3
from datetime import datetime, timedelta
import base64
import os
from dotenv import load_dotenv
import time
import threading

# Load environment variables
load_dotenv()

# GitHub API configuration
GITHUB_API_TOKEN = os.getenv('GITHUB_API_TOKEN')
GITHUB_API_URL = 'https://api.github.com'
HEADERS = {
    'Authorization': f'token {GITHUB_API_TOKEN}',
    'Accept': 'application/vnd.github.v3+json'
}

# Database configuration
DB_NAME = 'roc_corpus.db'

# Rate limiting configuration
RATE_LIMIT_SLEEP = int(os.getenv('RATE_LIMIT_SLEEP', 60))
class Db:
    def __init__(self, db_name):
        self.conn = sqlite3.connect(db_name)
        self.c = self.conn.cursor()
        self.lock = threading.Lock()
        self.last_commit_time = time.time()

    def _should_commit(self):
        current_time = time.time()
        if current_time - self.last_commit_time > 30:
            self.last_commit_time = current_time
            return True
        return False

    def add_repo(self, repo_url):
        with self.lock:
            self.c.execute('INSERT OR IGNORE INTO known_repos (repo_url) VALUES (?)', (repo_url,))
            if self._should_commit():
                self.conn.commit()

    def add_file(self, file_hash, commit_sha, file_contents, repo_url, file_path):
        with self.lock:
            self.c.execute('''INSERT INTO roc_files (file_hash, commit_sha, retrieval_date, file_contents, repo_url, file_path)
                             VALUES (?, ?, ?, ?, ?, ?)''',
                          (file_hash, commit_sha, datetime.now().isoformat(), file_contents, repo_url, file_path))
            if self._should_commit():
                self.conn.commit()

    def update_repo_scan_results(self, repo_url, scan_results):
        with self.lock:
            self.c.execute('INSERT INTO known_repo_scan_results (repo_url, scan_date, scan_results) VALUES (?, ?, ?)',
                          (repo_url, datetime.now().isoformat(), scan_results))
            if self._should_commit():
                self.conn.commit()

    def get_existing_repo_urls(self):
        with self.lock:
            self.c.execute('SELECT DISTINCT repo_url FROM roc_files')
            repo_urls = [row[0] for row in self.c.fetchall()]
        return repo_urls

    def get_repo_scan_info(self, repo_url):
        with self.lock:
            self.c.execute('SELECT scan_date, scan_results FROM known_repo_scan_results WHERE repo_url=? ORDER BY scan_date DESC LIMIT 1', (repo_url,))
            result = self.c.fetchone()
        return result

    def close(self):
        with self.lock:
            self.conn.close()

def create_db():
    db = Db(DB_NAME)
    db.c.execute('''CREATE TABLE IF NOT EXISTS roc_files
                 (id INTEGER PRIMARY KEY AUTOINCREMENT,
                  file_hash TEXT,
                  commit_sha TEXT,
                  retrieval_date TEXT,
                  file_contents TEXT,
                  repo_url TEXT,
                  file_path TEXT)''')
    db.c.execute('''CREATE TABLE IF NOT EXISTS known_repos
                 (id INTEGER PRIMARY KEY AUTOINCREMENT,
                  repo_url TEXT UNIQUE)''')
    db.c.execute('''CREATE TABLE IF NOT EXISTS known_repo_scan_results
                 (id INTEGER PRIMARY KEY AUTOINCREMENT,
                  repo_url TEXT,
                  scan_date TEXT,
                  scan_results INTEGER)''')
    db.conn.commit()
    db.close()

def get_existing_repo_urls(db):
    return db.get_existing_repo_urls()

def get_repo_scan_info(db, repo_url):
    return db.get_repo_scan_info(repo_url)

def update_repo_scan_info(db, repo_url, scan_results):
    db.update_repo_scan_results(repo_url, scan_results)

def scan_repo(repo_url):
    headers = {'Authorization': f'token {GITHUB_API_TOKEN}'}
    repo_owner, repo_name = repo_url.split('/')[-2:]

    # First, get the default branch
    repo_api_url = f'https://api.github.com/repos/{repo_owner}/{repo_name}'
    response = requests.get(repo_api_url, headers=headers)

    if response.status_code == 200:
        default_branch = response.json().get('default_branch', 'main')
        print(f"Default branch: {default_branch}")

        # Now, fetch the file tree using the default branch
        api_url = f'https://api.github.com/repos/{repo_owner}/{repo_name}/git/trees/{default_branch}?recursive=1'
        print(f"Requesting {api_url}")

        response = requests.get(api_url, headers=headers)
        if response.status_code == 200:
            files = response.json().get('tree', [])
            roc_files = [file for file in files if file['path'].endswith('.roc')]
            return roc_files
        else:
            print(f"Failed to fetch file tree for repo {repo_url}: {response.status_code}")
            return []
    else:
        print(f"Failed to fetch repo {repo_url}: {response.status_code}")
        return []

def insert_roc_files(db, repo_url, roc_files):
    if not roc_files:
        return

    for f in roc_files:
        db.add_file(f['sha'], '', '', repo_url, f['path'])

def should_scan_repo(last_scan_date, scan_results):
    if last_scan_date is None:
        return True
    last_scan = datetime.fromisoformat(last_scan_date)
    now = datetime.now()
    if scan_results > 0 and (now - last_scan) < timedelta(hours=24):
        return False
    if scan_results == 0 and (now - last_scan) < timedelta(days=30):
        return False
    return True

def search_roc_files():
    query = 'extension:roc -repo:roc-lang/roc'
    url = f'{GITHUB_API_URL}/search/code?q={query}&per_page=100'
    items = []
    while url:
        response = requests.get(url, headers=HEADERS)
        if response.status_code == 403 and 'X-RateLimit-Reset' in response.headers:
            reset_time = int(response.headers['X-RateLimit-Reset'])
            sleep_time = max(reset_time - time.time(), RATE_LIMIT_SLEEP)
            print(f"Rate limit exceeded. Sleeping for {sleep_time} seconds.")
            time.sleep(sleep_time)
            continue
        elif response.status_code != 200:
            response.raise_for_status()

        data = response.json()
        items.extend(data.get('items', []))
        url = data.get('next', None)

        remaining = response.headers.get('X-RateLimit-Remaining')
        reset_time = response.headers.get('X-RateLimit-Reset')
        if remaining and reset_time:
            reset_time = datetime.fromtimestamp(int(reset_time))
            print(f"Requests remaining: {remaining}, Reset time: {reset_time}")

    return items

def get_file_content(url):
    response = requests.get(url, headers=HEADERS)
    content = response.json()['content']
    return base64.b64decode(content).decode('utf-8')

def store_file(db, file_hash, commit_sha, file_contents, repo_url, file_path):
    db.add_file(file_hash, commit_sha, file_contents, repo_url, file_path)

def explore_via_search(db):
    roc_files = search_roc_files()
    for item in roc_files:
        file_url = item['url']
        commit_sha = item['sha']
        repo_url = item['repository']['html_url']
        file_path = item['path']

        file_contents = get_file_content(file_url)
        file_hash = hashlib.sha256(file_contents.encode('utf-8')).hexdigest()

        store_file(db, file_hash, commit_sha, file_contents, repo_url, file_path)
        print(f"Stored file: {item['repository']['full_name']} {file_path}")

def explore_via_known_repos(db):
    repo_urls = get_existing_repo_urls(db)

    for repo_url in repo_urls:
        scan_info = get_repo_scan_info(db, repo_url)
        if scan_info:
            last_scan_date, scan_results = scan_info
        else:
            last_scan_date, scan_results = None, 0

        if should_scan_repo(last_scan_date, scan_results):
            roc_files = scan_repo(repo_url)
            insert_roc_files(db, repo_url, roc_files)
            update_repo_scan_info(db, repo_url, len(roc_files))

def main():
    create_db()
    db = Db(DB_NAME)

    try:
        # explore_via_search(db)
        explore_via_known_repos(db)
    finally:
        db.commit()
        db.close()

if __name__ == '__main__':
    main()
