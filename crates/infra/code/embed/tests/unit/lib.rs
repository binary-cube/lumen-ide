use super::*;

/// With the model downloaded (e.g. `EDITOR_MODEL_DIR=…`): plausible similarity.
#[test]
#[ignore = "needs the downloaded model"]
fn real_model_ranks_related_text_higher() {
    let dir = std::env::var("EDITOR_MODEL_DIR").expect("EDITOR_MODEL_DIR");
    let model = Model2Vec::load(Path::new(&dir)).unwrap();
    let query = model.embed("send confirmation email to the user");
    let mail = model.embed("function sendWelcomeMail($user) { mail($user->email, 'Welcome'); }");
    let price = model.embed("function totalPrice($items) { return array_sum($items); }");
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    assert!(dot(&query, &mail) > dot(&query, &price));
}

/// A damaged model file is an error, never a crash: a header claiming more than the file
/// holds, offsets out of order.
#[test]
fn a_damaged_model_is_refused() {
    let dir = std::env::temp_dir().join(format!("embed-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tokenizer.json"), r#"{"model":{"vocab":{"a":0}}}"#).unwrap();
    let mut huge = u64::MAX.to_le_bytes().to_vec();
    huge.extend_from_slice(b"{}");
    std::fs::write(dir.join("model.safetensors"), &huge).unwrap();
    assert!(Model2Vec::load(&dir).is_err());
    let header = br#"{"embeddings":{"dtype":"F32","shape":[1,1],"data_offsets":[8,4]}}"#;
    let mut backwards = (header.len() as u64).to_le_bytes().to_vec();
    backwards.extend_from_slice(header);
    backwards.extend_from_slice(&[0; 16]);
    std::fs::write(dir.join("model.safetensors"), &backwards).unwrap();
    assert!(Model2Vec::load(&dir).is_err());
    assert!(!is_installed(&dir), "files of other sizes aren't the model");
    let _ = std::fs::remove_dir_all(dir);
}
