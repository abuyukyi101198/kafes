pub const SANDBOX_PATH: &str = "kafes/sandbox";
pub const BUN_PATH: &str = ".bun/bin/bun";
pub const BUN_VERSION: &str = "1.4.4";
pub const OMP_PATH: &str = ".bun/bin/omp";
pub const OMP_VERSION: &str = "18.16.0";
pub const GATEWAY_CFG_PATH: &str = ".config/kafes/gateway";
pub const TASKS_PATH: &str = "kafes/tasks";
pub const PUSH_CFG_PATH: &str = ".config/kafes/push";

pub const ACL: &str = "kafes-no-lan";
pub const ACL_RULES: [&[&str]; 4] = [
    &[
        "egress",
        "action=reject",
        "destination=10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,100.64.0.0/10",
    ],
    &["egress", "action=reject", "destination=fc00::/7,fe80::/10"],
    &["egress", "action=allow"],
    &["ingress", "action=allow"],
];

pub const UNIT_BROKER: &str = "kafes-broker";
pub const UNIT_GATEWAY: &str = "kafes-gateway";
pub const UNIT_PROXY: &str = "kafes-proxy";
pub const BROKER_URL: &str = "http://127.0.0.1:8765";

pub const ACTIONS_APP_ID: u64 = 15368;

pub const DEV_YAML: &str = include_str!("../assets/dev.yaml");
pub const POLICY: &str = include_str!("../assets/kafes-policy.yml");
pub const PROMPT: &str = include_str!("../assets/kafes-prompt.md");
pub const SETUP_WS: &str = include_str!("../assets/setup-workshop.sh");
pub const PROXY_TS: &str = include_str!("../assets/kafes-push-proxy.ts");
pub const CHECK_YML: &str = include_str!("../assets/kafes-pr-check.yml");

pub const PRESETS: &[(&str, &str)] = &[("yarn", include_str!("../assets/profiles/yarn.toml"))];

pub const STATE_PATH: &str = "kafes/sandbox/state";
pub const WS_HOME: &str = "/home/workshop";
