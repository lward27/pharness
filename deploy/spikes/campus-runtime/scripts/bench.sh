# CPU, syscall, process and file-I/O micro-benchmarks (python runner image).
cat >/tmp/files.py <<'PYEOF'
import os
import sys

root = "/workspace/files"
count = 10000
mode = sys.argv[1]
if mode == "create":
    os.makedirs(root, exist_ok=True)
    for i in range(count):
        with open(f"{root}/f{i:05d}", "wb") as handle:
            handle.write(b"x" * 128)
elif mode == "stat":
    total = 0
    for i in range(count):
        total += os.stat(f"{root}/f{i:05d}").st_size
    print(len(os.listdir(root)), total)
elif mode == "delete":
    for i in range(count):
        os.unlink(f"{root}/f{i:05d}")
PYEOF
phase cpu_python_loop python3 -c 'print(sum(i * i for i in range(20000000)))'
phase sha256_1gib_zero sh -c 'head -c 1073741824 /dev/zero | sha256sum'
phase syscall_getppid_1m python3 -c 'import os
for _ in range(1000000):
    os.getppid()'
phase fork_exec_true_1k sh -c 'i=0; while [ "$i" -lt 1000 ]; do /bin/true; i=$((i + 1)); done'
phase python_startup_20 sh -c 'i=0; while [ "$i" -lt 20 ]; do python3 -c pass; i=$((i + 1)); done'
phase files_create_10k python3 /tmp/files.py create
phase files_stat_10k python3 /tmp/files.py stat
phase files_delete_10k python3 /tmp/files.py delete
phase dd_write_512mib_fsync dd if=/dev/zero of=/workspace/dd.bin bs=1M count=512 conv=fsync
phase dd_read_512mib dd if=/workspace/dd.bin of=/dev/null bs=1M
rm -f /workspace/dd.bin
cgroup_report
