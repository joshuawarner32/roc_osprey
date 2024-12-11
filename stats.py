import sqlite3
import time

# db.c.execute('''CREATE TABLE IF NOT EXISTS roc_files
#              (id INTEGER PRIMARY KEY AUTOINCREMENT,
#               file_hash TEXT,
#               commit_sha TEXT,
#               retrieval_date TEXT,
#               file_contents TEXT,
#               repo_url TEXT,
#               file_path TEXT)''')

def get_db_stats(db_path):
    """Retrieve and print stats from the roc_files table."""
    conn = None
    try:
        # Connect to the SQLite database
        conn = sqlite3.connect(db_path)
        cursor = conn.cursor()

        # Query to count the number of rows in the table
        cursor.execute('SELECT COUNT(*) FROM roc_files')
        total_rows = cursor.fetchone()[0]
        print(f"Total rows: {total_rows}")

        # Query to get the size of the file_contents column for unique file_sha
        cursor.execute('SELECT SUM(LENGTH(file_contents)) FROM (SELECT DISTINCT file_hash, file_contents FROM roc_files)')
        total_content_size = cursor.fetchone()[0]
        print(f"Total file_contents size: {total_content_size} bytes")

        # File count by retrieval_date
        cursor.execute('SELECT DATE(retrieval_date), COUNT(*) FROM roc_files GROUP BY DATE(retrieval_date)')
        rows = cursor.fetchall()
        print("File count by retrieval_date:")
        for row in rows:
            print(f"{row[0]}: {row[1]}")

    except sqlite3.Error as e:
        print(f"SQLite error: {e}")
    finally:
        if conn:
            conn.close()

def main():
    db_path = 'roc_corpus.db'
    while True:
        get_db_stats(db_path)
        time.sleep(30)  # Sleep for 30 seconds

if __name__ == '__main__':
    main()
