use squirrel_lsp::semantic_analyzer::compute_semantic_tokens;

#[test]
fn test_multiline_comments_are_split_per_line() {
    let source = r#"/* Multi-line comment
   on multiple lines
   should be split */"#;

    let tokens = compute_semantic_tokens(source).expect("Failed to compute tokens");

    // Should have 3 tokens, one for each line of the comment
    assert_eq!(tokens.len(), 3);

    // All tokens should be comment type (17)
    for token in &tokens {
        assert_eq!(token.token_type, 17, "Token should be comment type");
    }

    // First token should be on line 0
    assert_eq!(tokens[0].delta_line, 0);
    assert_eq!(tokens[0].delta_start, 0);
    assert_eq!(tokens[0].length, 21); // "/* Multi-line comment"

    // Second token should be on line 1
    assert_eq!(tokens[1].delta_line, 1);
    assert_eq!(tokens[1].delta_start, 0);
    assert_eq!(tokens[1].length, 20); // "   on multiple lines"

    // Third token should be on line 2
    assert_eq!(tokens[2].delta_line, 1);
    assert_eq!(tokens[2].delta_start, 0);
    assert_eq!(tokens[2].length, 21); // "   should be split */"
}

#[test]
fn test_single_line_comment_works() {
    let source = "// Single line comment";

    let tokens = compute_semantic_tokens(source).expect("Failed to compute tokens");

    assert_eq!(tokens.len(), 1, "Single-line comment should be one token");
    assert_eq!(tokens[0].token_type, 17);
    assert_eq!(tokens[0].delta_line, 0);
    assert_eq!(tokens[0].delta_start, 0);
    assert_eq!(tokens[0].length, 22);
}

#[test]
fn test_inline_multiline_comment() {
    let source = "local x = 42; /* inline comment */";

    let tokens = compute_semantic_tokens(source).expect("Failed to compute tokens");

    // Find the comment token
    let comment_token = tokens
        .iter()
        .find(|t| t.token_type == 17)
        .expect("Should have a comment token");

    // Comment should be properly positioned
    assert_eq!(comment_token.length, 20);
}

#[test]
fn test_mixed_comments() {
    let source = r#"// Single line
/* Multi
   line */
local x = 1;"#;

    let tokens = compute_semantic_tokens(source).expect("Failed to compute tokens");

    // Count comment tokens (type 17)
    let comment_count = tokens.iter().filter(|t| t.token_type == 17).count();
    assert_eq!(comment_count, 3);
}
