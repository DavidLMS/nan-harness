/// Pi 1.1 config values use $NAME/${NAME} templates; $$ and $! escape literals.
pub(super) fn expand(value: &str, environment: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut remaining = value;
    let mut result = String::new();
    while let Some(index) = remaining.find('$') {
        result.push_str(&remaining[..index]);
        remaining = &remaining[index + 1..];
        if let Some(rest) = remaining
            .strip_prefix('$')
            .or_else(|| remaining.strip_prefix('!'))
        {
            result.push(remaining.chars().next()?);
            remaining = rest;
            continue;
        }
        let (name, consumed) = if let Some(braced) = remaining.strip_prefix('{') {
            match braced.find('}') {
                Some(end) if valid_name(&braced[..end]) => (&braced[..end], end + 2),
                _ => {
                    result.push('$');
                    continue;
                }
            }
        } else {
            let end = remaining
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            if !valid_name(&remaining[..end]) {
                result.push('$');
                continue;
            }
            (&remaining[..end], end)
        };
        result.push_str(&environment(name).filter(|value| !value.is_empty())?);
        remaining = &remaining[consumed..];
    }
    result.push_str(remaining);
    Some(result)
}

fn valid_name(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_native_templates_without_reinterpreting_literals_as_environment_names() {
        for (input, expected) in [
            ("KEY", "KEY"),
            ("${KEY}", "secret"),
            ("a-$KEY", "a-secret"),
            ("$$KEY", "$KEY"),
            ("$!command", "!command"),
            ("${not valid}", "${not valid}"),
        ] {
            assert_eq!(
                expand(input, |name| (name == "KEY").then(|| "secret".into())),
                Some(expected.into())
            );
        }
        assert_eq!(expand("${MISSING}", |_| None), None);
    }
}
