#[inline]
pub fn starts_with_ascii_alpha(string: &str) -> bool {
    matches!(string.as_bytes()[0], b'a'..=b'z' | b'A'..=b'Z')
}
#[derive(PartialEq, Eq)]
pub enum Context {
    UrlParser,
    Setter,
}
pub fn parse_scheme<'a>(input: &'a str, context: Context) -> Option<(String, &'a str)> {
    if input.is_empty() || !starts_with_ascii_alpha(input) {
        return None
    }
    for (i, c) in input.char_indices() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '+' | '-' | '.' => (),
            ':' => return Some((
                input[..i].to_ascii_lowercase(),
                &input[i + 1..],
            )),
            _ => return None,
        }
    }
    // EOF before ':'
    match context {
        Context::Setter => Some((input.to_ascii_lowercase(), "")),
        Context::UrlParser => None
    }
}
pub fn main(){
    parse_scheme("http:www.example.com",Context::UrlParser);
}