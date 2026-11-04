# Busybox CLI negative checks. Every probe is expected to be denied; none
# writes data to the host. Output format: "@@NEGTXT name :: output".
mkdir -p /tmp/mnt
probe() {
  _name=$1
  shift
  _out=$({ "$@" 2>&1; echo "rc=$?"; } | head -c 600 | tr '\n' ' ')
  echo "@@NEGTXT $_name :: $_out"
}
probe id id
probe uname uname -a
probe caps grep -E '^(Cap|NoNewPrivs|Seccomp)' /proc/self/status
probe mount_tmpfs mount -t tmpfs tmpfs /tmp/mnt
probe unshare_user unshare -r id
probe unshare_mount unshare -m true
probe nsenter_pid1 nsenter -t 1 -m true
probe hostname_set hostname luc114-probe
probe chroot chroot / /bin/true
probe insmod_devnull insmod /dev/null
probe module_tools sh -c 'for t in modprobe insmod rmmod; do command -v "$t" || echo "$t absent"; done'
probe dmesg dmesg
probe kcore_read sh -c 'head -c 16 /proc/kcore | od -An -c | head -n 2'
probe sysfs_open_for_append sh -c 'exec 3>>/sys/kernel/mm/transparent_hugepage/enabled && echo opened'
probe dev_kvm ls -l /dev/kvm
probe cpuinfo sh -c 'grep -m1 "model name" /proc/cpuinfo; grep -c ^processor /proc/cpuinfo'
