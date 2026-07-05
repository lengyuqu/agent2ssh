// V4-3: built-in config templates. Each applies a `policy.toml` (escalation
// rules layered on top of the built-in classifier — policy can only ESCALATE
// risk, never downgrade, so these read as "how much extra caution on top of
// the defaults") and an `execution_limits.toml`. Both are real, schema-valid
// TOML for AgentPolicyFile (policy.rs) / ExecutionLimitConfig (limits.rs) —
// applying a template writes these files directly, it isn't a cosmetic preview.
//
// Caveat surfaced in the UI, not hidden here: policy.toml is read through an
// mtime-checked cache, so it applies live; execution_limits.toml is only read
// once at daemon startup today, so a limits change needs a daemon restart.

export type ConfigTemplate = {
  id: string;
  name: string;
  description: string;
  policyToml: string;
  limitsToml: string;
};

const BASELINE_POLICY = `[risk.high]
patterns = ["systemctl stop *", "systemctl disable *", "docker rm *", "docker rmi *", "kill -9 *"]

[risk.medium]
patterns = ["apt-get remove *", "npm uninstall -g *", "pip uninstall *"]

[[approval.policies]]
name = "Baseline: approve high risk and above"
hosts = []
tags = []
min_risk = "high"
command_pattern = "*"
requires_approval = true
ttl_secs = 600
`;

const BASELINE_LIMITS = `enabled = true
window_secs = 60
default_source_per_minute = 30
default_host_per_minute = 20
default_tag_per_minute = 20
default_source_max_sessions = 5
default_host_max_sessions = 3
default_tag_max_sessions = 3
`;

const DEV_POLICY = `[risk.high]
patterns = []

[risk.medium]
patterns = []
`;

const DEV_LIMITS = `enabled = true
window_secs = 60
default_source_per_minute = 120
default_host_per_minute = 60
default_tag_per_minute = 60
default_source_max_sessions = 10
default_host_max_sessions = 6
default_tag_max_sessions = 6
`;

const PROD_POLICY = `[risk.high]
patterns = ["systemctl *", "service *", "docker *", "kubectl delete *", "kubectl apply *", "iptables *", "ufw *", "useradd *", "userdel *", "usermod *", "passwd *", "crontab *", "mount *", "umount *"]

[risk.medium]
patterns = ["apt-get *", "yum *", "npm install *", "pip install *"]

[[approval.policies]]
name = "Production: approve high risk and above"
hosts = []
tags = []
min_risk = "high"
command_pattern = "*"
requires_approval = true
ttl_secs = 300
`;

const PROD_LIMITS = `enabled = true
window_secs = 60
default_source_per_minute = 10
default_host_per_minute = 8
default_tag_per_minute = 8
default_source_max_sessions = 2
default_host_max_sessions = 2
default_tag_max_sessions = 2
`;

export const CONFIG_TEMPLATES: ConfigTemplate[] = [
  {
    id: "baseline",
    name: "Baseline security",
    description:
      "A sane default for most setups: escalates common service/package removal commands to high risk and requires approval above that.",
    policyToml: BASELINE_POLICY,
    limitsToml: BASELINE_LIMITS,
  },
  {
    id: "development",
    name: "Development",
    description:
      "Minimal extra rules on top of the built-in classifier, no mandatory approval, generous rate limits — for a sandbox/dev fleet.",
    policyToml: DEV_POLICY,
    limitsToml: DEV_LIMITS,
  },
  {
    id: "production",
    name: "Production operations",
    description:
      "Broad escalation for service/container/user/network-management commands, mandatory approval with a short TTL, and tight rate limits.",
    policyToml: PROD_POLICY,
    limitsToml: PROD_LIMITS,
  },
];
