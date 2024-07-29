import requests
import hashlib
import sqlite3
from datetime import datetime, timedelta
import base64
import os
import time
import threading
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

class Github:
    def __init__(self):
        self.api_token = os.getenv('GITHUB_API_TOKEN')
        self.headers = {
            'Authorization': f'token {self.api_token}',
            'Accept': 'application/vnd.github.v3+json'
        }
        self.api_url = 'https://api.github.com'
        self.db_name = 'githubcache.db'
        self.rate_limit_sleep = int(os.getenv('RATE_LIMIT_SLEEP', 60))
        self._create_db()
        self.lock = threading.Lock()

    def _create_db(self):
        with sqlite3.connect(self.db_name) as conn:
            cursor = conn.cursor()
            cursor.execute('''CREATE TABLE IF NOT EXISTS api_cache
                              (id INTEGER PRIMARY KEY AUTOINCREMENT,
                               url TEXT UNIQUE,
                               response TEXT,
                               timestamp DATETIME)''')
            conn.commit()

    def _is_cache_valid(self, timestamp):
        now = datetime.now()
        cache_time = datetime.fromisoformat(timestamp)
        return (now - cache_time) < timedelta(hours=24)

    def _get_cached_response(self, url):
        with sqlite3.connect(self.db_name) as conn:
            cursor = conn.cursor()
            cursor.execute('SELECT response, timestamp FROM api_cache WHERE url = ?', (url,))
            result = cursor.fetchone()
            if result and self._is_cache_valid(result[1]):
                return result[0]
        return None

    def _cache_response(self, url, response):
        with sqlite3.connect(self.db_name) as conn:
            cursor = conn.cursor()
            cursor.execute('''INSERT OR REPLACE INTO api_cache (url, response, timestamp)
                              VALUES (?, ?, ?)''', (url, response, datetime.now().isoformat()))
            conn.commit()

    def _request(self, method, url, **kwargs):
        response = self._get_cached_response(url)
        if response:
            return response

        while True:
            res = requests.request(method, url, headers=self.headers, **kwargs)
            if res.status_code == 403 and 'X-RateLimit-Reset' in res.headers:
                reset_time = int(res.headers['X-RateLimit-Reset'])
                sleep_time = max(reset_time - time.time(), self.rate_limit_sleep)
                print(f"Rate limit exceeded. Sleeping for {sleep_time} seconds.")
                time.sleep(sleep_time)
                continue
            elif res.status_code != 200:
                res.raise_for_status()

            response = res.text
            self._cache_response(url, response)
            return response

    def get(self, endpoint, **kwargs):
        url = f'{self.api_url}/{endpoint}'
        return self._request('GET', url, **kwargs)

# Usage example
if __name__ == '__main__':
    github = Github()
    response = github.get('repos/octocat/hello-world')
    print(response)
