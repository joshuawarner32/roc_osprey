import hashlib
import os

password = os.urandom(32).hex()
hashed_password = hashlib.sha256(password.encode()).hexdigest()
print(password)
print(hashed_password)
