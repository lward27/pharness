# Node lab: pinned clone, npm ci, npm test.
phase clone clone_at_sha || { cgroup_report; exit 10; }
cd /workspace/repo || exit 10
echo "@@REVISION $(git rev-parse HEAD)"
phase install npm ci --no-audit --no-fund || { cgroup_report; exit 12; }
TAIL_LINES=15
phase test npm test
rc=$?
cgroup_report
exit "$rc"
