use bar_threads::ThreadStatus;
use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq)]
pub struct AttentionAlert {
    pub thread_id: String,
    pub body: String,
    pub status: ThreadStatus,
}
impl AttentionAlert {
    pub fn title(&self) -> &'static str {
        match self.status {
            ThreadStatus::Approval => "Approval needed",
            ThreadStatus::Input => "Input needed",
            ThreadStatus::Request => "Guest request",
            ThreadStatus::Limited => "Usage limit reached",
            ThreadStatus::Failed => "Failed needed",
            _ => "Attention needed",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttentionTracker {
    known: Option<HashMap<String, ThreadStatus>>,
}
impl AttentionTracker {
    pub fn update<'a>(
        &mut self,
        threads: impl IntoIterator<Item = (&'a str, &'a str, ThreadStatus)>,
        muted: bool,
    ) -> Vec<AttentionAlert> {
        let threads: Vec<_> = threads.into_iter().collect();
        let current = threads
            .iter()
            .map(|(id, _, status)| (id.to_string(), *status))
            .collect();
        let previous = self.known.replace(current);
        let Some(previous) = previous.filter(|_| !muted) else {
            return vec![];
        };
        threads
            .into_iter()
            .filter_map(|(id, title, status)| {
                let needs_alert = matches!(
                    status,
                    ThreadStatus::Approval
                        | ThreadStatus::Input
                        | ThreadStatus::Request
                        | ThreadStatus::Failed
                        | ThreadStatus::Limited
                );
                (needs_alert && previous.get(id) != Some(&status)).then(|| AttentionAlert {
                    thread_id: id.into(),
                    body: title.into(),
                    status,
                })
            })
            .collect()
    }
}
