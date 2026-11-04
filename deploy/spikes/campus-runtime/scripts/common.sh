# Shared helpers prepended by driver.py to every shell workload.
set -u
export HOME="${HOME:-/workspace/home}"
mkdir -p "$HOME"

now() { date +%s.%N; }

# phase NAME CMD... runs CMD with output captured to a per-phase log, then
# prints a machine-readable timing line and the tail of the log.
phase() {
  _name=$1
  shift
  _start=$(now)
  "$@" >"/workspace/phase-$_name.log" 2>&1
  _rc=$?
  _end=$(now)
  echo "@@PHASE name=$_name rc=$_rc start=$_start end=$_end"
  tail -n "${TAIL_LINES:-20}" "/workspace/phase-$_name.log" | sed "s/^/[$_name] /"
  return "$_rc"
}

# A depth-1 fetch of one exact commit keeps the clone reproducible without
# depending on branch or tag names.
clone_at_sha() {
  git init -q /workspace/repo &&
    cd /workspace/repo &&
    git remote add origin "$LAB_REPO" &&
    git fetch -q --depth 1 origin "$LAB_SHA" &&
    git -c advice.detachedHead=false checkout -q FETCH_HEAD
}

# runc exposes the container's cgroup v2 files. gVisor exposes its own
# emulated cgroup v1 hierarchy, which accounts for the sandboxed workload only
# (not the Sentry and Gofer host processes).
cgroup_report() {
  for _f in memory.peak memory.current memory.max cpu.stat cpu.max \
    memory/memory.max_usage_in_bytes memory/memory.usage_in_bytes memory/memory.limit_in_bytes \
    cpuacct/cpuacct.usage; do
    if [ -r "/sys/fs/cgroup/$_f" ]; then
      echo "@@CGROUP $_f $(tr '\n' ' ' <"/sys/fs/cgroup/$_f")"
    else
      echo "@@CGROUP $_f unavailable"
    fi
  done
}

echo "@@START epoch=$(now) kernel=$(uname -r)"
