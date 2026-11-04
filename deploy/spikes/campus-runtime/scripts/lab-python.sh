# Python lab: pinned clone, venv, pip install, pytest.
phase clone clone_at_sha || { cgroup_report; exit 10; }
cd /workspace/repo || exit 10
echo "@@REVISION $(git rev-parse HEAD)"
phase venv python3 -m venv /workspace/venv || { cgroup_report; exit 11; }
phase install /workspace/venv/bin/pip install --no-input --progress-bar off -r requirements/tests.txt -e . || { cgroup_report; exit 12; }
TAIL_LINES=8
phase test /workspace/venv/bin/python -m pytest -q -p no:cacheprovider
rc=$?
cgroup_report
exit "$rc"
