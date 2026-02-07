use std::collections::BTreeMap;

/// Attribution captured for a single change event.
///
/// The dual attribution model allows one update to carry both a human actor
/// and an agent actor. When both are present, both should be credited.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateAttribution {
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
}

impl UpdateAttribution {
    pub fn for_agent(agent_id: impl Into<String>) -> Self {
        Self { agent_id: Some(agent_id.into()), ..Self::default() }
    }

    pub fn for_user(
        user_id: impl Into<String>,
        user_name: impl Into<String>,
        user_email: Option<String>,
    ) -> Self {
        Self {
            user_id: Some(user_id.into()),
            user_name: Some(user_name.into()),
            user_email,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoAuthor {
    pub name: String,
    pub email: String,
}

/// Collect unique co-authors from dual attribution updates since last commit.
///
/// Deduplication key is normalized email.
pub fn collect_coauthors_since_last_commit(updates: &[UpdateAttribution]) -> Vec<CoAuthor> {
    let mut by_email: BTreeMap<String, CoAuthor> = BTreeMap::new();

    for update in updates {
        if let Some(author) = coauthor_for_user(update) {
            by_email.entry(author.email.clone()).or_insert(author);
        }
        if let Some(author) = coauthor_for_agent(update) {
            by_email.entry(author.email.clone()).or_insert(author);
        }
    }

    by_email.into_values().collect()
}

/// Append `Co-authored-by` trailers to a commit message.
pub fn append_coauthor_trailers(base_message: &str, coauthors: &[CoAuthor]) -> String {
    if coauthors.is_empty() {
        return base_message.to_string();
    }

    let mut message = base_message.trim_end_matches('\n').to_string();
    message.push_str("\n\n");
    for author in coauthors {
        message.push_str(&format!("Co-authored-by: {} <{}>\n", author.name, author.email));
    }
    message
}

/// Convenience helper: collect + append trailers in one call.
pub fn with_coauthor_trailers(base_message: &str, updates: &[UpdateAttribution]) -> String {
    let coauthors = collect_coauthors_since_last_commit(updates);
    append_coauthor_trailers(base_message, &coauthors)
}

fn coauthor_for_user(update: &UpdateAttribution) -> Option<CoAuthor> {
    let user_id = normalize(update.user_id.as_deref())?;
    let name = normalize(update.user_name.as_deref()).unwrap_or(user_id).to_string();
    let email = normalize(update.user_email.as_deref())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("user:{user_id}@scriptum"));

    Some(CoAuthor { name, email })
}

fn coauthor_for_agent(update: &UpdateAttribution) -> Option<CoAuthor> {
    let agent_id = normalize(update.agent_id.as_deref())?;
    let name = normalize(update.agent_name.as_deref()).unwrap_or(agent_id).to_string();
    Some(CoAuthor { name, email: format!("agent:{agent_id}@scriptum") })
}

fn normalize(value: Option<&str>) -> Option<&str> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_unique_human_and_agent_coauthors() {
        let updates = vec![
            UpdateAttribution {
                user_id: Some("user-1".into()),
                user_name: Some("Gary".into()),
                user_email: Some("gary@example.com".into()),
                agent_id: Some("claude-1".into()),
                agent_name: Some("Claude".into()),
            },
            UpdateAttribution::for_agent("claude-1"),
            UpdateAttribution::for_user("user-2", "Dana", None),
        ];

        let coauthors = collect_coauthors_since_last_commit(&updates);
        assert_eq!(
            coauthors,
            vec![
                CoAuthor { name: "Claude".into(), email: "agent:claude-1@scriptum".into() },
                CoAuthor { name: "Gary".into(), email: "gary@example.com".into() },
                CoAuthor { name: "Dana".into(), email: "user:user-2@scriptum".into() },
            ]
        );
    }

    #[test]
    fn appends_coauthor_trailers_in_order() {
        let message = append_coauthor_trailers(
            "docs: update auth docs",
            &[
                CoAuthor { name: "Gary".into(), email: "gary@example.com".into() },
                CoAuthor { name: "Claude".into(), email: "agent:claude-1@scriptum".into() },
            ],
        );

        assert_eq!(
            message,
            "docs: update auth docs\n\n\
             Co-authored-by: Gary <gary@example.com>\n\
             Co-authored-by: Claude <agent:claude-1@scriptum>\n"
        );
    }

    #[test]
    fn ignores_blank_identifiers() {
        let updates = vec![
            UpdateAttribution {
                user_id: Some("   ".into()),
                user_name: Some("Human".into()),
                user_email: Some("human@example.com".into()),
                agent_id: None,
                agent_name: None,
            },
            UpdateAttribution {
                user_id: None,
                user_name: None,
                user_email: None,
                agent_id: Some("   ".into()),
                agent_name: Some("Agent".into()),
            },
        ];

        assert!(collect_coauthors_since_last_commit(&updates).is_empty());
    }

    #[test]
    fn with_coauthor_trailers_keeps_base_message_when_no_authors() {
        let result = with_coauthor_trailers("chore: checkpoint", &[]);
        assert_eq!(result, "chore: checkpoint");
    }
}
