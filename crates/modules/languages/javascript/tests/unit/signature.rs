use super::*;

#[test]
fn js_parameters_read_and_written_back() {
    for text in ["a", "b = 1", "...rest", "opts: Options = {}", "{ a, b }"] {
        assert_eq!(JsParams.render(&JsParams.parse(text)), text);
    }
}
