#!/bin/sh
# CNB #28 — which capability lets root read another uid's /proc/<pid>/environ.
# Run as root in a disposable Linux container (default /proc mount, hidepid=0).
# The target is a stand-in carrying a synthetic marker; only marker presence is printed.
set -eu
MARK=A2S_LINUX_ENV_C7E2
useradd -M -u 1005 a2s1005
A2S_SYNTH=$MARK setpriv --reuid=1005 --regid=1005 --clear-groups sleep 60 &
P=$!
sleep 0.5
echo "kernel: $(uname -r)  proc: $(grep ' /proc ' /proc/self/mountinfo | sed 's/.* - //')"
echo "target pid=$P $(grep '^Uid:' /proc/$P/status | tr '\t' ' ')  environ mode: $(stat -c '%a' /proc/$P/environ)"
for drop in none -sys_ptrace -dac_override,-dac_read_search -sys_ptrace,-dac_override,-dac_read_search; do
  if [ "$drop" = none ]; then pre=""; else pre="setpriv --bounding-set=$drop --inh-caps=-all"; fi
  $pre sh -c "
    printf 'root dropped=[%s] CapEff=%s environ=' '$drop' \$(grep CapEff /proc/self/status | cut -f2)
    if out=\$(tr '\\0' '\\n' 2>&1 < /proc/$P/environ); then
      if echo \"\$out\" | grep -q $MARK; then echo marker-visible; else echo read-ok-no-marker; fi
    else echo denied; fi" 2>&1 | sed 's/sh: [0-9]*: //'
done
kill $P; wait $P 2>/dev/null || true
userdel a2s1005
