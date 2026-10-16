#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputKind {
    Edit,
    Document,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub runtime_id: Option<Vec<i32>>,
    pub kind: InputKind,
    pub name: String,
    pub automation_id: String,
    pub help_text: String,
    pub class_name: String,
    pub read_only: bool,
    pub has_value_pattern: bool,
    pub has_text_edit_pattern: bool,
    pub focused: bool,
}

fn score(candidate: &Candidate) -> Option<i32> {
    let attributes = format!("{} {}", candidate.automation_id, candidate.class_name).to_lowercase();
    let name = candidate.name.trim().to_lowercase();
    let identity = format!(
        "{} {} {}",
        candidate.name, candidate.automation_id, candidate.class_name
    )
    .to_lowercase();
    if candidate.read_only
        || candidate.automation_id.eq_ignore_ascii_case("RootWebArea")
        || [
            "search",
            "搜索",
            "查找",
            "filter",
            "筛选",
            "command-palette",
        ]
        .iter()
        .any(|key| attributes.contains(key) || name.starts_with(key))
    {
        return None;
    }
    let hints = format!("{identity} {}", candidate.help_text).to_lowercase();
    let semantic = [
        (
            80,
            &[
                "composer",
                "chat-input",
                "chatinput",
                "prosemirror",
                "tiptap",
                "lexical",
            ] as &[_],
        ),
        (60, &["message", "消息", "询问", "提问", "prompt"] as &[_]),
        (40, &["ask", "输入", "问题", "textarea", "textbox"] as &[_]),
    ]
    .into_iter()
    .find_map(|(value, keys)| keys.iter().any(|key| hints.contains(key)).then_some(value))
    .unwrap_or(0);
    if candidate.kind == InputKind::Document
        && semantic == 0
        && !candidate.has_value_pattern
        && !candidate.has_text_edit_pattern
    {
        return None;
    }
    Some(
        semantic
            + if candidate.kind == InputKind::Edit {
                20
            } else {
                0
            }
            + if candidate.has_text_edit_pattern {
                5
            } else {
                0
            }
            + if candidate.has_value_pattern { 3 } else { 0 }
            + if candidate.focused { 1 } else { 0 },
    )
}

pub fn choose(candidates: &[Candidate], cached_id: Option<&[i32]>) -> Result<usize, String> {
    let eligible: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| score(candidate).map(|value| (index, value)))
        .collect();
    if let Some(id) = cached_id {
        if let Some((index, _)) = eligible
            .iter()
            .find(|(index, _)| candidates[*index].runtime_id.as_deref() == Some(id))
        {
            return Ok(*index);
        }
    }
    let best_score = eligible.iter().map(|(_, value)| *value).max();
    let mut best = eligible
        .iter()
        .filter(|(_, value)| Some(*value) == best_score);
    if let Some((index, _)) = best.next() {
        if best.next().is_none() {
            return Ok(*index);
        }
    }
    Err("无法自动确定聊天输入框。请确认目标窗口已打开聊天页面后重试。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor(name: &str, id: i32) -> Candidate {
        Candidate {
            runtime_id: Some(vec![42, id]),
            kind: InputKind::Edit,
            name: name.into(),
            automation_id: String::new(),
            help_text: String::new(),
            class_name: String::new(),
            read_only: false,
            has_value_pattern: true,
            has_text_edit_pattern: false,
            focused: false,
        }
    }
    #[test]
    fn finds_composer_without_any_previous_binding() {
        assert_eq!(
            choose(&[editor("Search", 1), editor("Message", 2)], None),
            Ok(1)
        );
    }
    #[test]
    fn stale_binding_automatically_uses_rebuilt_composer() {
        assert_eq!(
            choose(
                &[editor("Search", 1), editor("Message", 20)],
                Some(&[42, 2])
            ),
            Ok(1)
        );
    }
    #[test]
    fn valid_cached_input_is_reused() {
        assert_eq!(
            choose(&[editor("Message", 1), editor("", 2)], Some(&[42, 2])),
            Ok(1)
        );
    }
    #[test]
    fn cached_search_field_or_read_only_control_is_not_reused() {
        let mut readonly = editor("Message", 2);
        readonly.read_only = true;
        let items = [editor("Search", 1), readonly, editor("Message", 3)];
        assert_eq!(choose(&items, Some(&[42, 1])), Ok(2));
        assert_eq!(choose(&items, Some(&[42, 2])), Ok(2));
    }
    #[test]
    fn mentioning_search_in_editor_label_does_not_turn_it_into_a_search_box() {
        let mut composer = editor("Message: please search the code", 1);
        composer.class_name = "ProseMirror".into();
        assert_eq!(choose(&[composer], None), Ok(0));
    }
    #[test]
    fn document_root_is_not_mistaken_for_the_prompt_input() {
        let mut root = editor("Prompt composer", 1);
        root.kind = InputKind::Document;
        root.automation_id = "RootWebArea".into();
        assert_eq!(choose(&[root, editor("", 2)], None), Ok(1));
    }
    #[test]
    fn editable_control_does_not_need_to_expose_its_text() {
        let mut item = editor("Message", 1);
        item.has_value_pattern = false;
        assert_eq!(choose(&[item], None), Ok(0));
    }
    #[test]
    fn rich_composer_is_identified_by_class_name() {
        let mut rich = editor("", 2);
        rich.kind = InputKind::Document;
        rich.has_value_pattern = false;
        rich.class_name = "tiptap ProseMirror".into();
        assert_eq!(choose(&[editor("Name", 1), rich], None), Ok(1));
    }
    #[test]
    fn ambiguous_or_missing_inputs_return_a_discovery_error() {
        assert!(choose(&[editor("", 1), editor("", 2)], Some(&[42, 3])).is_err());
        assert!(choose(&[], None).is_err());
    }
}
