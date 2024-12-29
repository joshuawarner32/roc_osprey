#!/usr/bin/env python3

import zulip
import os
import json
from typing import Any
import sqlite3
import time

class CachedZulipClient:
    def __init__(self, config_file: str):
        self.client = zulip.Client(config_file=config_file)
        self.conn = sqlite3.connect('zulip_cache.db')
        self.create_cache_table()
        self._last_commit_time = time.time()

    def create_cache_table(self):
        with self.conn:
            self.conn.execute('''
                CREATE TABLE IF NOT EXISTS cache (
                    method TEXT,
                    params TEXT,
                    response TEXT,
                    PRIMARY KEY (method, params)
                )
            ''')

    def get_cached_response(self, method: str, params: dict) -> Any:
        cursor = self.conn.cursor()
        cursor.execute('SELECT response FROM cache WHERE method = ? AND params = ?', (method, json.dumps(params)))
        row = cursor.fetchone()
        return json.loads(row[0]) if row else None

    def cache_response(self, method: str, params: dict, response: Any):
        current_time = time.time()
        with self.conn:
            self.conn.execute('''
                INSERT OR REPLACE INTO cache (method, params, response) VALUES (?, ?, ?)
            ''', (method, json.dumps(params), json.dumps(response)))

            if current_time - self._last_commit_time > 30:
                self.conn.commit()
                self._last_commit_time = current_time

    def __getattr__(self, name: str):
        original_method = getattr(self.client, name)

        def cached_method(*args, **kwargs):
            params = {'args': args, 'kwargs': kwargs}
            cached_response = self.get_cached_response(name, params)
            if cached_response is not None:
                return cached_response

            response = original_method(*args, **kwargs)
            self.cache_response(name, params, response)
            return response

        return cached_method

# Read the zuliprc file from the same directory as this script
client = CachedZulipClient(config_file=os.path.join(os.path.dirname(__file__), ".zuliprc"))
result = client.get_subscriptions()
assert result["result"] == "success", result.get("msg", result)

# # Get the 100 last messages sent by "iago@zulip.com" to
# # the channel named "Verona".
# request: dict[str, Any] = {
#     "anchor": "newest",
#     "num_before": 100,
#     "num_after": 0,
#     "narrow": [
#         {"operator": "channel", "operand": "Verona"},
#     ],
# }
# result = client.get_messages(request)
# print(result)


with sqlite3.connect('zulip_code_blocks.db') as conn:
    conn.execute('''
        CREATE TABLE IF NOT EXISTS messages (
            channel TEXT,
            message_id INTEGER PRIMARY KEY,
            content TEXT
        )
    ''')

    for sub in result["subscriptions"]:
        print(sub["name"])

        anchor = "newest"
        for _ in range(100):
            request = {
                "anchor": anchor,
                "num_before": 1000,
                "num_after": 0,
                "narrow": [
                    {"operator": "channel", "operand": sub["name"]},
                ],
            }
            messages_result = client.get_messages(request)

            if messages_result["result"] == "success":
                messages = messages_result["messages"]
                if not messages:
                    break

                for message in messages:
                    content = message["content"]
                    if "```" in content or "code" in content or "pre" in content or "github.com" in content:
                        conn.execute('''
                            INSERT OR REPLACE INTO messages (channel, message_id, content) VALUES (?, ?, ?)
                        ''', (sub["name"], message["id"], content))

                anchor = messages[0]["id"] - 1
            else:
                print(messages_result)
                break
