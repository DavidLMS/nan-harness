use super::*;

pub(crate) fn prepare_documents(
    plans: &[DocumentPlan],
    previous: Option<&[DocumentReceipt]>,
) -> Result<Vec<PreparedDocument>, ConfigurationError> {
    let mut matched = BTreeSet::new();
    let prepared = plans
        .iter()
        .map(|plan| {
            let previous = previous.and_then(|receipts| {
                receipts.iter().enumerate().find_map(|(index, receipt)| {
                    (!matched.contains(&index) && plan_matches_receipt(plan, receipt))
                        .then_some((index, receipt))
                })
            });
            if let Some((index, _)) = previous {
                matched.insert(index);
            }
            let previous = previous.map(|(_, receipt)| receipt);
            match (plan, previous) {
                (DocumentPlan::Json(plan), None) => prepare_json(plan, None),
                (DocumentPlan::Json(plan), Some(DocumentReceipt::Json(receipt))) => {
                    prepare_json(plan, Some(receipt))
                }
                (DocumentPlan::Yaml(plan), None) => prepare_yaml(plan, None),
                (DocumentPlan::Yaml(plan), Some(DocumentReceipt::Yaml(receipt))) => {
                    prepare_yaml(plan, Some(&PreviousYamlReceipt::Yaml(receipt)))
                }
                (DocumentPlan::Yaml(plan), Some(DocumentReceipt::TextBlock(receipt))) => {
                    prepare_yaml(plan, Some(&PreviousYamlReceipt::TextBlock(receipt)))
                }
                (DocumentPlan::TextBlock(plan), None) => prepare_text_block(plan, None),
                (DocumentPlan::TextBlock(plan), Some(DocumentReceipt::TextBlock(receipt))) => {
                    prepare_text_block(plan, Some(receipt))
                }
                (DocumentPlan::ExactFile(plan), None) => prepare_exact_file(plan, None),
                (DocumentPlan::ExactFile(plan), Some(DocumentReceipt::ExactFile(receipt))) => {
                    prepare_exact_file(plan, Some(receipt))
                }
                (DocumentPlan::Kimi(plan), None) => prepare_kimi(plan, None),
                (DocumentPlan::Kimi(plan), Some(DocumentReceipt::Toml(receipt))) => {
                    prepare_kimi(plan, Some(receipt))
                }
                _ => Err(ConfigurationError::ReceiptMismatch),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if previous.is_some_and(|receipts| matched.len() != receipts.len()) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    Ok(prepared)
}

pub(crate) fn plan_matches_receipt(plan: &DocumentPlan, receipt: &DocumentReceipt) -> bool {
    match (plan, receipt) {
        (DocumentPlan::Json(plan), DocumentReceipt::Json(receipt)) => plan.path == receipt.path,
        (DocumentPlan::Yaml(plan), DocumentReceipt::Yaml(receipt)) => plan.path == receipt.path,
        (DocumentPlan::Yaml(plan), DocumentReceipt::TextBlock(receipt)) => {
            plan.path == receipt.path
                && plan.legacy_block.as_ref().is_some_and(|legacy| {
                    legacy.begin == receipt.begin && legacy.end == receipt.end
                })
        }
        (DocumentPlan::TextBlock(plan), DocumentReceipt::TextBlock(receipt)) => {
            plan.path == receipt.path && plan.begin == receipt.begin && plan.end == receipt.end
        }
        (DocumentPlan::ExactFile(plan), DocumentReceipt::ExactFile(receipt)) => {
            plan.path == receipt.path
        }
        (DocumentPlan::Kimi(plan), DocumentReceipt::Toml(receipt)) => plan.path == receipt.path,
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PreviousYamlReceipt<'a> {
    Yaml(&'a YamlReceipt),
    TextBlock(&'a TextBlockReceipt),
}

pub(crate) fn prepare_removals(
    receipts: &[DocumentReceipt],
) -> Result<Vec<PreparedDocument>, ConfigurationError> {
    receipts
        .iter()
        .map(|receipt| match receipt {
            DocumentReceipt::Json(receipt) => prepare_json_removal(receipt),
            DocumentReceipt::Yaml(receipt) => prepare_yaml_removal(receipt),
            DocumentReceipt::TextBlock(receipt) => prepare_text_block_removal(receipt),
            DocumentReceipt::ExactFile(receipt) => prepare_exact_file_removal(receipt),
            DocumentReceipt::Toml(receipt) => prepare_kimi_removal(receipt),
        })
        .collect()
}
