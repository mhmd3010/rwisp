#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resources {
    pub uptime: Option<String>,
    pub version: Option<String>,
    pub board_name: Option<String>,
    pub architecture_name: Option<String>,
    pub cpu: Option<String>,
    pub cpu_count: Option<u32>,
    pub cpu_frequency: Option<u32>,
    pub cpu_load: Option<u32>,
    pub free_memory: Option<u64>,
    pub total_memory: Option<u64>,
    pub free_hdd_space: Option<u64>,
    pub total_hdd_space: Option<u64>,
}
