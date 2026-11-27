#[cfg(test)]
mod tests {
    use cf_base::env::Env;
    use cf_engine::roles::role_instructions;
    use cf_proto::cf_word::CfWord;

    #[test]
    fn a_removed_eval_card_variable_cannot_replace_the_chiefs_instructions() {
        let folder = cf_e2e::door::made("t-").unwrap();
        let missing = folder.path().join("missing.md");
        let existing = folder.path().join("card.md");
        cf_e2e::door::write(&existing, "replacement text").unwrap();
        let expected = role_instructions(&Env::default(), "chief", &[], &CfWord::bare(), None)
            .expect("ordinary instructions");
        assert!(expected.contains("You do not change the project yourself"));
        for card in [&existing, &missing] {
            let env = Env::from_vars([("CONSENSFLOW_EVAL_CHIEF_CARD", card.as_os_str())]);
            let got = role_instructions(&env, "chief", &[], &CfWord::bare(), None)
                .expect("the removed variable must not be read");
            assert_eq!(got, expected);
        }
    }
}
