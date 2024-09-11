use crate::error::{Error, Result};
use crate::{bert, jtalk, model, nlp, norm, style, tokenizer, utils};
use ndarray::{concatenate, s, Array, Array1, Array2, Axis};
use ort::Session;
use tokenizers::Tokenizer;

#[derive(PartialEq, Eq, Clone)]
pub struct TTSIdent(String);

impl std::fmt::Display for TTSIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)?;
        Ok(())
    }
}

impl<S> From<S> for TTSIdent
where
    S: AsRef<str>,
{
    fn from(value: S) -> Self {
        TTSIdent(value.as_ref().to_string())
    }
}

pub struct TTSModel {
    vits2: Session,
    style_vectors: Array2<f32>,
    ident: TTSIdent,
}

pub struct TTSModelHolder {
    tokenizer: Tokenizer,
    bert: Session,
    models: Vec<TTSModel>,
    jtalk: jtalk::JTalk,
}

impl TTSModelHolder {
    pub fn new<P: AsRef<[u8]>>(bert_model_bytes: P, tokenizer_bytes: P) -> Result<Self> {
        let bert = model::load_model(bert_model_bytes)?;
        let jtalk = jtalk::JTalk::new()?;
        let tokenizer = tokenizer::get_tokenizer(tokenizer_bytes)?;
        Ok(TTSModelHolder {
            bert,
            models: vec![],
            jtalk,
            tokenizer,
        })
    }

    pub fn models(&self) -> Vec<String> {
        self.models.iter().map(|m| m.ident.to_string()).collect()
    }

    pub fn load<I: Into<TTSIdent>, P: AsRef<[u8]>>(
        &mut self,
        ident: I,
        style_vectors_bytes: P,
        vits2_bytes: P,
    ) -> Result<()> {
        let ident = ident.into();
        if self.find_model(ident.clone()).is_err() {
            self.models.push(TTSModel {
                vits2: model::load_model(vits2_bytes)?,
                style_vectors: style::load_style(style_vectors_bytes)?,
                ident,
            })
        }
        Ok(())
    }

    pub fn unload<I: Into<TTSIdent>>(&mut self, ident: I) -> bool {
        let ident = ident.into();
        if let Some((i, _)) = self
            .models
            .iter()
            .enumerate()
            .find(|(_, m)| m.ident == ident)
        {
            self.models.remove(i);
            true
        } else {
            false
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn parse_text(
        &self,
        text: &str,
    ) -> Result<(Array2<f32>, Array1<i64>, Array1<i64>, Array1<i64>)> {
        let normalized_text = norm::normalize_text(text);

        let (phones, tones, mut word2ph) = self.jtalk.g2p(&normalized_text)?;
        let (phones, tones, lang_ids) = nlp::cleaned_text_to_sequence(phones, tones);

        let phones = utils::intersperse(&phones, 0);
        let tones = utils::intersperse(&tones, 0);
        let lang_ids = utils::intersperse(&lang_ids, 0);
        for item in &mut word2ph {
            *item *= 2;
        }
        word2ph[0] += 1;
        let (token_ids, attention_masks) = tokenizer::tokenize(&normalized_text, &self.tokenizer)?;

        let bert_content = bert::predict(&self.bert, token_ids, attention_masks)?;

        assert!(
            word2ph.len() == normalized_text.chars().count() + 2,
            "{} {}",
            word2ph.len(),
            normalized_text.chars().count()
        );

        let mut phone_level_feature = vec![];
        for (i, reps) in word2ph.iter().enumerate() {
            let repeat_feature = {
                let (reps_rows, reps_cols) = (*reps, 1);
                let arr_len = bert_content.slice(s![i, ..]).len();

                let mut results: Array2<f32> =
                    Array::zeros((reps_rows as usize, arr_len * reps_cols));

                for j in 0..reps_rows {
                    for k in 0..reps_cols {
                        let mut view = results.slice_mut(s![j, k * arr_len..(k + 1) * arr_len]);
                        view.assign(&bert_content.slice(s![i, ..]));
                    }
                }
                results
            };
            phone_level_feature.push(repeat_feature);
        }
        let phone_level_feature = concatenate(
            Axis(0),
            &phone_level_feature
                .iter()
                .map(|x| x.view())
                .collect::<Vec<_>>(),
        )?;
        let bert_ori = phone_level_feature.t();
        Ok((
            bert_ori.to_owned(),
            phones.into(),
            tones.into(),
            lang_ids.into(),
        ))
    }

    fn find_model<I: Into<TTSIdent>>(&self, ident: I) -> Result<&TTSModel> {
        let ident = ident.into();
        self.models
            .iter()
            .find(|m| m.ident == ident)
            .ok_or(Error::ModelNotFoundError(ident.to_string()))
    }

    pub fn get_style_vector<I: Into<TTSIdent>>(
        &self,
        ident: I,
        style_id: i32,
        weight: f32,
    ) -> Result<Array1<f32>> {
        style::get_style_vector(&self.find_model(ident)?.style_vectors, style_id, weight)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn synthesize<I: Into<TTSIdent>>(
        &self,
        ident: I,
        bert_ori: Array2<f32>,
        phones: Array1<i64>,
        tones: Array1<i64>,
        lang_ids: Array1<i64>,
        style_vector: Array1<f32>,
        sdp_ratio: f32,
        length_scale: f32,
    ) -> Result<Vec<u8>> {
        let buffer = model::synthesize(
            &self.find_model(ident)?.vits2,
            bert_ori.to_owned(),
            phones,
            tones,
            lang_ids,
            style_vector,
            sdp_ratio,
            length_scale,
        )?;
        Ok(buffer)
    }
}
