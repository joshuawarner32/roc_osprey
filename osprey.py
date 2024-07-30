import hashlib
import sqlite3
from datetime import datetime, timedelta
import base64
import os
import time
import threading
from dotenv import load_dotenv
from urllib.parse import urlparse
from github import Github
import requests
import zipfile
from io import BytesIO


# Load environment variables
load_dotenv()

# Database configuration
DB_NAME = 'roc_corpus.db'

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

    def add_file(self, file_hash, commit_sha, file_contents, repo_url, file_path):
        with self.lock:
            self.c.execute('''INSERT INTO roc_files (file_hash, commit_sha, retrieval_date, file_contents, repo_url, file_path)
                             VALUES (?, ?, ?, ?, ?, ?)''',
                          (file_hash, commit_sha, datetime.now().isoformat(), file_contents, repo_url, file_path))
            if self._should_commit():
                self.conn.commit()

    def update_repo_scan_results(self, repo_url, scan_sha, scan_results):
        with self.lock:
            self.c.execute('INSERT INTO repo_scan_results (repo_url, scan_sha, scan_date, scan_results) VALUES (?, ?, ?, ?)',
                          (repo_url, scan_sha, datetime.now().isoformat(), scan_results))
            if self._should_commit():
                self.conn.commit()

    def get_existing_repo_urls(self):
        with self.lock:
            self.c.execute('SELECT DISTINCT repo_url FROM roc_files')
            repo_urls = [row[0] for row in self.c.fetchall()]
        return repo_urls

    def get_repo_scan_info(self, repo_url):
        with self.lock:
            self.c.execute('SELECT scan_date, scan_sha, scan_results FROM repo_scan_results WHERE repo_url=? ORDER BY scan_date DESC LIMIT 1', (repo_url,))
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
    db.c.execute('''CREATE TABLE IF NOT EXISTS repo_scan_results
                 (id INTEGER PRIMARY KEY AUTOINCREMENT,
                  repo_url TEXT,
                  scan_date TEXT,
                  scan_sha TEXT,
                  scan_results INTEGER)''')
    db.conn.commit()
    db.close()

def scan_repo(github, repo_url, last_scan_sha):
    repo_owner, repo_name = repo_url.split('/')[-2:]

    # Get the default branch
    repo_info = github.get(f'repos/{repo_owner}/{repo_name}')
    default_branch = repo_info.get('default_branch', 'main')

    # Resolve the branch to the current commit hash
    branch_info = github.get(f'repos/{repo_owner}/{repo_name}/branches/{default_branch}')
    commit_sha = branch_info['commit']['sha']

    # Don't scan again if the previous sha matches
    if commit_sha == last_scan_sha:
        return None

    # Fetch the file tree using the commit hash
    files = github.get(f'repos/{repo_owner}/{repo_name}/git/trees/{commit_sha}?recursive=1')
    roc_files = [file for file in files['tree'] if file['path'].endswith('.roc')]
    return roc_files, commit_sha

def insert_roc_files(db, github, repo_url, repo_owner, repo_name, commit_sha, roc_files):
    if not roc_files:
        return
        if len(roc_files) > 3:
            print(f"Downloading {len(roc_files)} files from {repo_url} via zip")
            # Download the repository as a zip file
            repo_zip_url = f'https://github.com/{repo_owner}/{repo_name}/archive/{commit_sha}.zip'
            response = requests.get(repo_zip_url)
            response.raise_for_status()

            # Read the zip file directly from response content
            with zipfile.ZipFile(BytesIO(response.content)) as zip_ref:
                # Read the files directly from the zip archive
                for f in roc_files:
                    with zip_ref.open(f"{repo_name}-{commit_sha}/{f['path']}") as file:
                        file_contents = file.read().decode('utf-8')
                    db.add_file(f['sha'], commit_sha, file_contents, repo_url, f['path'])
        else:
            for f in roc_files:
                # Use the API to fetch the file contents
                if f["size"] == 0:
                    file_contents = ''
                else:
                    result = github.get_prefixed(f['url'])
                    assert result['sha'] == f['sha']
                    assert result['size'] == f['size']
                    assert result['encoding'] == 'base64'
                    file_contents = base64.b64decode(result['content']).decode('utf-8')
                db.add_file(f['sha'], commit_sha, file_contents, repo_url, f['path'])

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

def search_roc_files(github):
    query = 'extension:roc -repo:roc-lang/roc'
    items = []
    page = 1
    while True:
        result = github.get(f'search/code?q={query}&per_page=100&page={page}')
        items.extend(result['items'])
        if 'next' not in result:
            break
        page += 1
    return items

def get_file_content(github, url):
    file_info = github.get_prefixed(url)
    content = file_info['content']
    return base64.b64decode(content).decode('utf-8')

def explore_via_search(db, github):
    roc_files = search_roc_files(github)
    for item in roc_files:
        file_url = item['url']
        commit_sha = item['sha']
        repo_url = item['repository']['html_url']
        file_path = item['path']

        file_contents = get_file_content(github, file_url)
        file_hash = hashlib.sha256(file_contents.encode('utf-8')).hexdigest()

        db.add_file(file_hash, commit_sha, file_contents, repo_url, file_path)
        print(f"Stored file: {item['repository']['full_name']} {file_path}")

def explore_via_known_repos(db, github):
    repo_urls = db.get_existing_repo_urls()
    explore_repos(db, github, repo_urls)

def explore_repos(db, github, repo_urls):
    for repo_url in repo_urls:
        parsed_url = urlparse(repo_url)
        assert parsed_url.netloc == 'github.com'
        repo_owner, repo_name = parsed_url.path.split('/')[1:3]

        scan_info = db.get_repo_scan_info(repo_url)
        if scan_info:
            last_scan_date, last_scan_sha, scan_results = scan_info
        else:
            last_scan_date, last_scan_sha, scan_results = None, None, 0

        if should_scan_repo(last_scan_date, scan_results):
            try:
                result = scan_repo(github, repo_url, last_scan_sha)
                if result is not None:
                    roc_files, commit_sha = result
                    insert_roc_files(db, github, repo_url, repo_owner, repo_name, commit_sha, roc_files)
                    db.update_repo_scan_results(repo_url, commit_sha, len(roc_files))
            except requests.exceptions.HTTPError as e:
                if e.response.status_code == 404:
                    print(f"Repository {repo_url} not found")
                    db.update_repo_scan_results(repo_url, None, -1)
                else:
                    print(f"Error scanning repository {repo_url}: {e}")
                    raise
            except Exception as e:
                print(f"Error scanning repository {repo_url}: {e}")
                raise

def explore_via_known_users(db, github):
    repo_urls = db.get_existing_repo_urls()
    users = set()

    for repo_url in repo_urls:
        try:
            parsed_url = urlparse(repo_url)
            assert parsed_url.netloc == 'github.com'
            user = parsed_url.path.split('/')[1]
            users.add(user)
        except Exception as e:
            print(f"Error parsing repo URL {repo_url}: {e}")

    repo_urls = set()
    print("Unique users from known repositories:")
    for user in sorted(users):
        print(user)
        try:
            page = 1
            while True:
                repos = github.get(f'users/{user}/repos?per_page=100&page={page}')
                if not repos:
                    break
                for repo in repos:
                    # print(f"  - {repo['full_name']}")
                    repo_urls.add(repo['html_url'])
                page += 1
        except Exception as e:
            print(f"Error fetching repos for user {user}: {e}")

    explore_repos(db, github, repo_urls)

def main():
    create_db()
    db = Db(DB_NAME)
    github = Github()

    try:
        explore_via_search(db, github)
        explore_via_known_repos(db, github)
        explore_via_known_users(db, github)
    finally:
        db.close()

if __name__ == '__main__':
    main()
