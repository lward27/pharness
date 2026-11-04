#!/usr/bin/env python3
"""Change the sole PHarness console user's password without printing credentials."""
import base64
import getpass
import json
import subprocess
import sys
import urllib.error
import urllib.request

try:
    import bcrypt
except ImportError:
    sys.exit('Install the Python bcrypt package before running this helper.')

KUBECTL = ['kubectl', '--context', 'lucas_engineering', '-n', 'pharness', '--request-timeout=15s']
NAME = 'pharness-console-basic-auth'

def kubectl(*args, input_data=None):
    result = subprocess.run(KUBECTL + list(args), input=input_data, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False)
    if result.returncode:
        raise RuntimeError('kubectl failed; check context, permissions, and cluster access')
    return result.stdout

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

def main():
    secret = json.loads(kubectl('get', 'secret', NAME, '-o', 'json'))
    if secret.get('type') != 'Opaque' or set(secret.get('data', {})) != {'auth'}:
        raise RuntimeError('Secret shape changed; refusing to replace existing credentials')
    if secret.get('immutable'):
        raise RuntimeError('Secret is immutable; no change made')
    lines = [line for line in base64.b64decode(secret['data']['auth'], validate=True).decode().splitlines() if line]
    if len(lines) != 1 or ':' not in lines[0]:
        raise RuntimeError('Expected exactly one existing htpasswd user; no change made')
    username, old_hash = lines[0].split(':', 1)
    if not username or not old_hash or ':' in username:
        raise RuntimeError('Invalid existing htpasswd entry; no change made')
    password = getpass.getpass('New console password (input hidden): ')
    if password != getpass.getpass('Repeat new password: '):
        raise RuntimeError('Passwords differ; no change made')
    password_bytes = password.encode('utf-8')
    if len(password_bytes) < 12 or len(password_bytes) > 72 or any(c in password for c in '\r\n\0'):
        raise RuntimeError('Use 12 to 72 UTF-8 bytes without newlines or NUL; no change made')
    hashed = bcrypt.hashpw(password_bytes, bcrypt.gensalt(rounds=12))
    entry = username.encode() + b':' + hashed + b'\n'
    patch = json.dumps({'metadata': {'resourceVersion': secret['metadata']['resourceVersion']},
                        'data': {'auth': base64.b64encode(entry).decode()}}).encode()
    options = ('patch', 'secret', NAME, '--type=merge', '--patch-file=/dev/stdin', '-o', 'name')
    kubectl(*options, '--dry-run=server', input_data=patch)
    try:
        kubectl(*options, input_data=patch)
    except RuntimeError as exc:
        raise RuntimeError('Password update outcome is uncertain; inspect the Secret before retrying') from exc
    print('Password updated. Checking the HTTPS console with the new credentials...')
    auth = base64.b64encode(username.encode() + b':' + password_bytes).decode()
    request = urllib.request.Request('https://pharness.lucas.engineering/', headers={'Authorization': 'Basic ' + auth})
    try:
        with urllib.request.build_opener(NoRedirect()).open(request, timeout=20) as response:
            if response.status != 200:
                raise RuntimeError(f'Console returned HTTP {response.status}')
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f'Password was updated, but console verification returned HTTP {exc.code}; do not repeat blindly') from exc
    except urllib.error.URLError as exc:
        raise RuntimeError('Password was updated, but console verification could not connect; do not repeat blindly') from exc
    print('Verified: console returned HTTP 200. Use your existing username and the password you just entered.')

if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError, UnicodeError) as exc:
        print(f'Error: {exc}', file=sys.stderr)
        sys.exit(1)
