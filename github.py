import requests
import sqlite3
from datetime import datetime, timedelta
import os
import json
import time
import threading
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

# Known route mappings for rate limit kinds
RATE_LIMIT_KINDS = {
    "core": ["/repos", "/users", "/orgs", "/issues"],
    "search": ["/search"],
    "graphql": ["/graphql"]
}

# TokenBucket implementation
class NotYet(Exception):
    def __init__(self, sleep_time):
        self.sleep_time = sleep_time
        super().__init__(f"Need to wait {sleep_time:.2f} seconds.")

class TokenBucket:
    def __init__(self, capacity: float, rate: float, min_quota=10):
        self._max = float(capacity)
        self._capacity = float(capacity)
        self._rate = float(rate)
        self._tokens = float(capacity)
        self._min_quota = float(min_quota)
        self._last_update = time.time()

    def _update_tokens(self, now: float):
        elapsed = now - self._last_update
        if elapsed > 0:
            added = elapsed * self._rate
            self._tokens = min(self._tokens + added, self._capacity)
            self._last_update = now

    def grab_token(self):
        now = time.time()
        self._update_tokens(now)
        if self._tokens >= 1:
            self._tokens -= 1
            return
        else:
            needed = 1 - self._tokens
            sleep_time = needed / self._rate
            raise NotYet(sleep_time)

    def grab_token_blocking(self):
        while True:
            try:
                self.grab_token()
                return
            except NotYet as e:
                time.sleep(e.sleep_time)

    def update_quota(self, current_quota: float, reset_time: float):
        now = time.time()
        time_left = reset_time - now
        if time_left < 0:
            time_left = 1.0

        tokens_we_can_spend = max(0, current_quota - 1)
        new_rate = tokens_we_can_spend / time_left if time_left > 0 else 0

        self._update_tokens(now)
        self._capacity = float(current_quota)
        self._rate = float(new_rate)
        if self._tokens > self._capacity:
            self._tokens = self._capacity

# Updated Github class
class Github:
    def __init__(self):
        self.api_token = os.getenv('GITHUB_API_TOKEN')
        self.headers = {
            'Authorization': f'token {self.api_token}',
            'Accept': 'application/vnd.github.v3+json'
        }
        self.api_url = 'https://api.github.com'
        self.db_name = 'githubcache.db'
        self.conn = sqlite3.connect(self.db_name, check_same_thread=False)
        self._create_db()
        self.lock = threading.Lock()

        # Initialize token buckets for each rate limit type
        self.token_buckets = {
            "core": TokenBucket(5000, 5000 / 3600),
            "search": TokenBucket(10, 10 / 60),
            "graphql": TokenBucket(5000, 5000 / 3600)
        }

    def _create_db(self):
        cursor = self.conn.cursor()
        cursor.execute('''CREATE TABLE IF NOT EXISTS api_cache
                          (id INTEGER PRIMARY KEY AUTOINCREMENT,
                           url TEXT UNIQUE,
                           response TEXT,
                           timestamp DATETIME)''')
        cursor.execute('''CREATE INDEX IF NOT EXISTS idx_url ON api_cache (url)''')
        cursor.execute('''CREATE INDEX IF NOT EXISTS idx_timestamp_url ON api_cache (timestamp, url)''')
        cursor.execute('''DELETE FROM api_cache WHERE timestamp < ?''', ((datetime.now() - timedelta(hours=48)).isoformat(),))
        self.conn.commit()

    def _is_cache_valid(self, timestamp):
        now = datetime.now()
        cache_time = datetime.fromisoformat(timestamp)
        return (now - cache_time) < timedelta(hours=48)

    def _cache_response(self, url, response):
        cursor = self.conn.cursor()
        cursor.execute('''INSERT OR REPLACE INTO api_cache (url, response, timestamp)
                          VALUES (?, ?, ?)''', (url, response, datetime.now().isoformat()))
        self.conn.commit()

    def _get_rate_limit_kind(self, endpoint):
        for kind, routes in RATE_LIMIT_KINDS.items():
            if any(endpoint.startswith(route) for route in routes):
                return kind
        return "core"

    def _request(self, method, url):
        endpoint = url.replace(self.api_url, "")
        rate_limit_kind = self._get_rate_limit_kind(endpoint)
        bucket = self.token_buckets[rate_limit_kind]

        print(f"Checking cache for {url}")
        cursor = self.conn.cursor()
        cursor.execute('SELECT response, timestamp FROM api_cache WHERE url = ?', (url,))
        result = cursor.fetchone()
        if result and self._is_cache_valid(result[1]):
            return json.loads(result[0])

        while True:
            bucket.grab_token_blocking()
            print(f"Requesting {url}")
            try:
                res = requests.request(method, url, headers=self.headers, timeout=10)
                reset_time = int(res.headers['X-RateLimit-Reset'])
                limit = float(res.headers['X-RateLimit-Limit'])
                assert limit == bucket._max, f"Expected {bucket._max} but got {limit} for route {url}"
                remaining = int(res.headers.get('X-RateLimit-Remaining', 0))
                bucket.update_quota(remaining, reset_time)
                if res.status_code == 403 and 'X-RateLimit-Reset' in res.headers and res.headers.get('X-RateLimit-Remaining') == '0':
                    print(f"Rate limit exceeded. Sleeping until {reset_time}")
                    time.sleep(reset_time - time.time())
                    continue
                elif res.status_code != 200:
                    res.raise_for_status()
            except requests.exceptions.RequestException as e:
                print(f"Request error: {e}. Retrying...")
                time.sleep(5)
                continue

            self._cache_response(url, res.text)
            return res.json()

    def get(self, endpoint):
        url = f'{self.api_url}/{endpoint}'
        return self._request('GET', url)

    def get_prefixed(self, url):
        assert url.startswith(self.api_url)
        return self._request('GET', url)
