use log::warn;

#[derive(Clone, Copy, Debug)]
pub enum AbstractDomainType {
    Interval,
}
#[derive(Clone, Debug)]
pub struct AnalysisOption {
    pub entry_point: String,
    pub entry_def_id_index: Option<u32>,
    pub domain_type: AbstractDomainType,
    pub widening_delay: u32,
    pub cleaning_delay: usize,
    pub narrowing_iteration: u32,
    pub show_entries: bool,
    pub show_entries_index: bool,
    pub deny_warnings: bool,
    pub memory_safety_only: bool,
    pub suppressed_warnings: usize,
}
impl Default for AnalysisOption {
    fn default() -> Self {
        Self {
            entry_point: String::from("main"),
            entry_def_id_index: None,
            domain_type: AbstractDomainType::Interval,
            widening_delay: 5,
            cleaning_delay: 5,
            narrowing_iteration: 5,
            show_entries: false,
            show_entries_index: false,
            deny_warnings: false,
            memory_safety_only: false,
            suppressed_warnings: None,
        }
    }
}
impl AnalysisOption {
    fn get_domain_type(arg: &str) -> Option<AbstractDomainType> {
        match arg {
            "interval" => Some(AbstractDomainType::Interval),
                _ => None,
            }
        }
}
    

