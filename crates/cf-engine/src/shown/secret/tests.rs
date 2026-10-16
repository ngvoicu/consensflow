//! Which lines name a secret, and which do not, by the words of both reviews of rounds 3
//! and 4.

use super::*;

#[test]
fn a_name_s_parts_and_the_words_of_a_line_are_told_apart() {
    assert_eq!(words_of("DB_PASSWORD=x"), ["db", "password", "x"]);
    assert_eq!(words_of("OAuth apiKey"), ["o", "auth", "api", "key"]);
    assert_eq!(words_of("x-api-key: 1"), ["x", "api", "key", "1"]);
    assert_eq!(
        words_of("HTTPServer password2"),
        ["http", "server", "password", "2"]
    );
    assert_eq!(words_of("API    key:\tabc"), ["api", "key", "abc"]);
}

#[test]
fn a_line_s_names_are_joined_whatever_their_case_or_split() {
    assert_eq!(
        names_of("MYSQL_PWD=\"red green\" x-api-key PassWord: 1").collect::<Vec<_>>(),
        ["mysqlpwd", "red", "green", "xapikey", "password", "1"]
    );
}

/// The fourth reviews' lines, whose values the masker misses (short words), and the
/// lead's: a password's name in a name of any case or split, `pwd`, and API and private
/// keys in the plural.
#[test]
fn a_password_or_a_key_named_anywhere_in_a_name_or_in_the_plural_is_named() {
    for line in [
        "DB_PWD=red green blue",
        "API keys: abcdefghijklmno",
        "MYSQL_PWD=\"red green blue\"",
        "PassWord=\"alpha bravo charlie\"",
        "dbPassword: alpha bravo charlie",
        "Pass_Word: alpha",
        "pwd",
        "PWD=/home/alice",
        "apikeys: alpha",
        "ApiKeys",
        "privatekeys",
        "PRIVATE_KEYS=alpha",
        "private keys: alpha",
        "x-api-keys: alpha",
        "PassPhrase",
        "userCredential=alpha",
        "SUPERPASSWORD=alpha",
    ] {
        assert!(names_a_secret(line), "{line}");
    }
}

#[test]
fn a_secret_named_as_a_word_or_as_a_part_of_a_name_is_named() {
    for line in [
        "DB_PASSWORD=hunter2",
        "OAuth failed",
        "Authorization: Basic xyz",
        "apiKey: abc",
        "x-api-key: abc",
        "API    key: abcdefghijklmno",
        "private_key_path: /tmp/id_rsa",
        "PrivateKey",
        "Invalid token: expired",
        "AccessToken",
        "credentials.json",
        "client_secret: x",
        "PASSWD=x",
        "Passphrase please",
        "BEARER abc",
        "auth_header",
        "export APIKEY=x",
    ] {
        assert!(names_a_secret(line), "{line}");
    }
}

#[test]
fn a_word_a_secret_s_name_is_a_piece_of_names_none() {
    for line in [
        "author",
        "Author identity unknown",
        "TypeError: author is undefined at src/author.ts:42",
        "panicked at src/author.rs:7",
        "secretary of state",
        "the tokenizer failed",
        "key = \"Escape\"",
        "key_file: /home/alice/.ssh/id_ed25519",
        "const monkey = \"banana\";",
        "private repository",
        "api ready",
        "SECRETARY_NAME=alice",
        "TokenizerConfig",
        "AUTHOR_EMAIL=alice@example.com",
        "tokenization",
        "press one of the keys",
    ] {
        assert!(!names_a_secret(line), "{line}");
    }
}
