#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub current_id: Option<String>,
    pub parent_id: Option<String>,
    pub branch_point: Option<usize>,
    pub has_branches: bool,
}
