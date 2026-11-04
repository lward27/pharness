#!/usr/bin/env python3
"""Bounded API-driven conference demonstrations; see the companion runbook."""
import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid

API = os.environ.get('PHARNESS_DEMO_API', 'http://127.0.0.1:4777').rstrip('/')
UI = os.environ.get('PHARNESS_DEMO_UI', 'http://127.0.0.1:5173').rstrip('/')
TOKEN = os.environ.get('PHARNESS_API_TOKEN', '')

def request(path, body=None):
    headers = {'Accept': 'application/json'}
    if TOKEN:
        headers['Authorization'] = 'Bearer ' + TOKEN
    data = None
    if body is not None:
        headers['Content-Type'] = 'application/json'
        data = json.dumps(body).encode()
    req = urllib.request.Request(API + path, data=data, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=40) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f'HTTP {exc.code} on {path}: {exc.read(4096).decode(errors="replace")}') from exc
    except (urllib.error.URLError, TimeoutError) as exc:
        raise RuntimeError(f'API request failed on {path}: {exc}; a POST may have succeeded, inspect audit before retrying') from exc

def show(value):
    print(json.dumps(value, indent=2, sort_keys=True))

def page(name):
    config = request('/api/config/effective')
    modern = config.get('features', {}).get('repo_mode_v1', {}).get('ui_enabled') is True
    routes = {'audit': ('insights/audit', 'audit'), 'status': ('settings/platform', 'status'), 'observations': ('insights/observations', 'observations'), 'releases': ('releases', 'work-items')}
    return UI + '/#/' + routes[name][0 if modern else 1]

def capability(action):
    action['id'] = 'conference.' + uuid.uuid4().hex
    action['reason'] = 'Conference demonstration of a typed capability'
    result = request('/api/capabilities/execute', {'action': action, 'timeout_ms': 5000})
    show(result)
    print('\nAudit page:', page('audit'))
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('step', choices=['preflight', 'allow', 'deny', 'audit', 'release'])
    parser.add_argument('--release-id', help='Existing approved/completed WorkItem-backed dev Release')
    args = parser.parse_args()
    if args.step == 'preflight':
        health = request('/health')
        config = request('/api/config/effective')
        readiness = request('/api/system/readiness')
        show({'health': health, 'policy': config.get('policy'), 'worker': config.get('worker'), 'operational_mode': readiness.get('operational_mode')})
        if not health.get('ok') or config.get('policy', {}).get('deny_secret_access') is not True or readiness.get('operational_mode') != 'normal':
            raise RuntimeError('preflight failed: health, secret-denial policy, or operational mode')
        print('\nOpen:', page('status'))
    elif args.step == 'allow':
        result = capability({'action': 'registry_inspect_image', 'image_ref': 'team/checkout-api:v0.1.0-smoke', 'registry_base_url': None})
        if result.get('status') != 'ok' or result.get('executed') is not True:
            raise RuntimeError('allow step did not execute successfully')
        print('Reference parsing only: no registry digest or provenance was verified.')
    elif args.step == 'deny':
        result = capability({'action': 'kubernetes_get', 'resource': 'secrets', 'namespace': 'default', 'name': None, 'all_namespaces': False, 'label_selector': None})
        if result.get('status') != 'denied' or result.get('executed') is not False:
            raise RuntimeError('denial did not produce the expected non-execution result')
    elif args.step == 'audit':
        data = request('/api/audit-events?resource_kind=capability&limit=100')
        events = [e for e in data.get('events', []) if e.get('payload', {}).get('action_id', '').startswith('conference.')]
        show({'conference_events': events})
        print('\nOpen:', page('audit'))
        if not events:
            raise RuntimeError('no conference capability events in the latest 100 records')
    else:
        if not args.release_id:
            parser.error('release requires --release-id')
        rid = urllib.parse.quote(args.release_id, safe='')
        release = request('/api/releases/' + rid)
        if release.get('status') not in ('approved', 'completed') or release.get('target_environment') != 'dev':
            raise RuntimeError('Release must be approved/completed and target dev; inspect the full delivery chain first')
        print('Selected Release:')
        show(release)
        print('Requesting read-only post-sync verification; this persists observations and audit records.')
        result = request('/api/releases/' + rid + '/verify', {'complete': False, 'actor': 'conference-demo', 'reason': 'Conference read-only verification', 'timeout_ms': 30000})
        show(result)
        print('\nOpen:', page('releases'), 'and', page('observations'))
        if result.get('verified') is not True:
            raise RuntimeError('verification did not pass; retain the returned evidence and do not claim readiness')

if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError) as exc:
        print('ERROR:', exc, file=sys.stderr)
        sys.exit(1)
