use crate::INDEX_DIR_NAME;
use crate::api_config::{ApiConfig, API_CONFIG_FILE_NAME, ApiConfigRaw};
use crate::chunk::{self, Chunk, ChunkBuildInfo, CHUNK_DIR_NAME};
use crate::error::Error;
use crate::prompts::{PROMPTS, PROMPT_DIR};
use crate::query::{Keywords, QueryConfig, QUERY_CONFIG_FILE_NAME, extract_keywords};
use crate::uid::{self, Uid};
use ragit_api::{
    Model,
    ModelRaw,
    RecordAt,
    Request,
};
use ragit_fs::{
    WriteMode,
    create_dir_all,
    exists,
    extension,
    into_abs_path,
    is_dir,
    join,
    join3,
    normalize,
    parent,
    read_bytes,
    read_dir,
    read_string,
    remove_file,
    set_extension,
    write_bytes,
    write_string,
};
use ragit_pdl::{
    Pdl,
    encode_base64,
    parse_pdl,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod commands;
mod config;
pub mod file;
mod ii;
pub mod tfidf;

pub use commands::{
    AddMode,
    AddResult,
    CloneResult,
    MergeMode,
    MergeResult,
    RecoverResult,
    VersionInfo,
    get_compatibility_warning,
};
pub use config::{BuildConfig, BUILD_CONFIG_FILE_NAME};
pub use file::{FileReader, ImageDescription};
pub use ii::IIStatus;
pub use tfidf::{ProcessedDoc, TfidfResult, TfidfState, consume_processed_doc};

pub const CONFIG_DIR_NAME: &str = "configs";
pub const II_DIR_NAME: &str = "ii";
pub const IMAGE_DIR_NAME: &str = "images";
pub const FILE_INDEX_DIR_NAME: &str = "files";
pub const INDEX_FILE_NAME: &str = "index.json";
pub const MODEL_FILE_NAME: &str = "models.json";
pub const LOG_DIR_NAME: &str = "logs";

pub type Path = String;

// all the `Path` are normalized relative paths
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Index {
    ragit_version: String,
    pub chunk_count: usize,
    pub staged_files: Vec<Path>,
    pub processed_files: HashMap<Path, Uid>,

    // Previously, all the builds were in serial and this field tells
    // which file the index is building. When something goes wrong, ragit
    // reads this field and clean up garbages. Now, all the builds are in
    // parallel and there's no such thing like `curr_processing_file`. But
    // we still need to tell whether something went wrong while building
    // and this field does that. If it's `Some(_)`, something's wrong and
    // clean-up has to be done.
    pub curr_processing_file: Option<Path>,
    repo_url: Option<String>,

    /// `ii` stands for `inverted-index`.
    pub ii_status: IIStatus,

    // it's not used by code, but used by serde
    // users modify json file, which is deserialized to `ApiConfigRaw`,
    // which is then converted to `ApiConfig` by `.init_api_config()`
    #[serde(skip)]
    api_config_raw: ApiConfigRaw,

    #[serde(skip)]
    pub root_dir: Path,
    #[serde(skip)]
    pub build_config: BuildConfig,
    #[serde(skip)]
    pub query_config: QueryConfig,
    #[serde(skip)]
    pub api_config: ApiConfig,
    #[serde(skip)]
    pub prompts: HashMap<String, String>,
    #[serde(skip)]
    pub models: Vec<Model>,
}

/// 1. If you want to do something with chunks, use `LoadMode::QuickCheck`.
/// 2. If you have nothing to do with chunks, use `LoadMode::OnlyJson`.
/// 3. If something's broken and you don't want it to crash, use `LoadMode::Minimum`. It can still crash, though.
/// 4. If you want to be very sure that nothing's broken and you don't care about init-time, use `LoadMode::Check`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoadMode {
    /// It only loads `index.json`. It doesn't care whether config files prompt files, or chunk files are broken.
    Minimum,

    /// It loads json files, but doesn't care whether the chunk files are broken.
    OnlyJson,

    /// It checks and auto-recovers if `self.curr_processing_file` is not None. If the value is not None,
    /// a previous build was interrupted and something could be broken.
    QuickCheck,

    /// It always checks and auto-recovers. You should be very careful, `check` and `auto-recover` are very expensive.
    Check,
}

impl Index {
    /// It works like git. `root_dir` is the root of the repo. And it creates dir `.ragit/`, like `.git/`.
    /// It reads the files in the repo and creates index.
    pub fn new(
        root_dir: Path,
    ) -> Result<Self, Error> {
        let index_dir = join(&root_dir, INDEX_DIR_NAME)?;
        let root_dir = normalize(&into_abs_path(&root_dir)?)?;

        if exists(&index_dir) {
            return Err(Error::IndexAlreadyExists(index_dir));
        }

        create_dir_all(&index_dir)?;

        for dir in [
            CONFIG_DIR_NAME,
            CHUNK_DIR_NAME,
            IMAGE_DIR_NAME,
            FILE_INDEX_DIR_NAME,
            II_DIR_NAME,
        ] {
            create_dir_all(&Index::get_rag_path(
                &root_dir,
                &dir.to_string(),
            )?)?;
        }

        let build_config = BuildConfig::default();
        let query_config = QueryConfig::default();
        let api_config = ApiConfig::default();
        let api_config_raw = ApiConfigRaw::default();

        let result = Index {
            ragit_version: crate::VERSION.to_string(),
            chunk_count: 0,
            staged_files: vec![],
            processed_files: HashMap::new(),
            curr_processing_file: None,
            build_config,
            query_config,
            api_config_raw,
            api_config,
            root_dir,
            repo_url: None,
            ii_status: IIStatus::None,
            prompts: PROMPTS.clone(),
            models: Model::default_models(),
        };

        write_bytes(
            &result.get_build_config_path()?,
            &serde_json::to_vec_pretty(&result.build_config)?,
            WriteMode::AlwaysCreate,
        )?;
        write_bytes(
            &result.get_query_config_path()?,
            &serde_json::to_vec_pretty(&result.query_config)?,
            WriteMode::AlwaysCreate,
        )?;
        write_bytes(
            &result.get_api_config_path()?,
            &serde_json::to_vec_pretty(&result.api_config_raw)?,
            WriteMode::AlwaysCreate,
        )?;
        result.save_to_file()?;

        Ok(result)
    }

    pub fn load(
        root_dir: Path,
        load_mode: LoadMode,
    ) -> Result<Self, Error> {
        let mut result = Index::load_minimum(root_dir)?;

        if load_mode == LoadMode::Minimum {
            return Ok(result);
        }

        result.build_config = serde_json::from_str::<BuildConfig>(
            &read_string(&result.get_build_config_path()?)?,
        )?;
        result.query_config = serde_json::from_str::<QueryConfig>(
            &read_string(&result.get_query_config_path()?)?,
        )?;
        result.api_config_raw = serde_json::from_str::<ApiConfigRaw>(
            &read_string(&result.get_api_config_path()?)?,
        )?;
        result.api_config = result.init_api_config(&result.api_config_raw)?;
        result.load_prompts()?;
        result.load_or_init_models()?;

        match load_mode {
            LoadMode::QuickCheck if result.curr_processing_file.is_some() => {
                result.recover()?;
                result.save_to_file()?;
                Ok(result)
            },
            LoadMode::Check if result.curr_processing_file.is_some() || result.check().is_err() => {
                result.recover()?;
                result.save_to_file()?;
                Ok(result)
            },
            _ => Ok(result),
        }
    }

    /// It only loads `index.json`. No config files, no prompt files, and it doesn't care whether chunk files are broken or not.
    /// It's for `rag check --recover`: it only loads minimum data and the recover function will load or fix the others.
    fn load_minimum(root_dir: Path) -> Result<Self, Error> {
        let root_dir = normalize(&into_abs_path(&root_dir)?)?;
        let index_json = read_string(&Index::get_rag_path(
            &root_dir,
            &INDEX_FILE_NAME.to_string(),
        )?)?;

        let mut result = serde_json::from_str::<Index>(&index_json)?;
        result.root_dir = root_dir;

        if let Some(warn) = get_compatibility_warning(&result.ragit_version, crate::VERSION) {
            eprintln!("Warning: {warn}");
        }

        Ok(result)
    }

    pub fn load_or_init(
        root_dir: Path,
    ) -> Result<Self, Error> {
        let index_dir = join(
            &root_dir,
            &INDEX_DIR_NAME.to_string(),
        )?;

        if exists(&index_dir) {
            // `load_or_init` cannot be done in only-json mode, because only-json `init` doesn't make sense
            Index::load(root_dir, LoadMode::QuickCheck)
        }

        else {
            Index::new(root_dir)
        }
    }

    pub fn save_to_file(&self) -> Result<(), Error> {
        self.save_prompts()?;

        Ok(write_bytes(
            &Index::get_rag_path(
                &self.root_dir,
                &INDEX_FILE_NAME.to_string(),
            )?,
            &serde_json::to_vec_pretty(self)?,
            WriteMode::Atomic,
        )?)
    }

    pub(crate) async fn load_chunks_or_tfidf(
        &self,
        query: &str,
    ) -> Result<Vec<Chunk>, Error> {
        if self.chunk_count > self.query_config.max_titles {
            let keywords = extract_keywords(self, query).await?;
            let tfidf_results = self.run_tfidf(
                keywords,
                self.query_config.max_summaries,
            )?;
            let mut chunks = Vec::with_capacity(tfidf_results.len());

            for tfidf_result in tfidf_results.into_iter() {
                let uid = tfidf_result.id;
                chunks.push(self.get_chunk_by_uid(uid)?);
            }

            Ok(chunks)
        }

        else {
            let mut chunks = vec![];

            for chunk_path in self.get_all_chunk_files()? {
                chunks.push(chunk::load_from_file(&chunk_path)?);
            }

            Ok(chunks)
        }
    }

    pub fn get_all_chunk_files(&self) -> Result<Vec<Path>, Error> {
        let mut result = vec![];

        for internal in read_dir(&join3(&self.root_dir, &INDEX_DIR_NAME, &CHUNK_DIR_NAME)?, false)? {
            if !is_dir(&internal) {
                continue;
            }

            for chunk_file in read_dir(&internal, false)? {
                if extension(&chunk_file).unwrap_or(None).unwrap_or(String::new()) == "chunk" {
                    result.push(chunk_file.to_string());
                }
            }
        }

        // the result has to be deterministic
        result.sort();
        Ok(result)
    }

    pub fn get_all_tfidf_files(&self) -> Result<Vec<Path>, Error> {
        let mut result = vec![];

        for internal in read_dir(&join3(&self.root_dir, &INDEX_DIR_NAME, &CHUNK_DIR_NAME)?, false)? {
            if !is_dir(&internal) {
                continue;
            }

            for tfidf_file in read_dir(&internal, false)? {
                if extension(&tfidf_file).unwrap_or(None).unwrap_or(String::new()) == "tfidf" {
                    result.push(tfidf_file.to_string());
                }
            }
        }

        // the result has to be deterministic
        result.sort();
        Ok(result)
    }

    pub fn get_all_image_files(&self) -> Result<Vec<Path>, Error> {
        let mut result = vec![];

        for internal in read_dir(&join3(&self.root_dir, &INDEX_DIR_NAME, &IMAGE_DIR_NAME)?, false)? {
            if !is_dir(&internal) {
                continue;
            }

            for image_file in read_dir(&internal, false)? {
                if extension(&image_file).unwrap_or(None).unwrap_or(String::new()) == "png" {
                    result.push(image_file.to_string());
                }
            }
        }

        // the result has to be deterministic
        result.sort();
        Ok(result)
    }

    fn get_all_file_indexes(&self) -> Result<Vec<Path>, Error> {
        let mut result = vec![];

        for internal in read_dir(&join3(&self.root_dir, &INDEX_DIR_NAME, &FILE_INDEX_DIR_NAME)?, false)? {
            if !is_dir(&internal) {
                continue;
            }

            for file_index in read_dir(&internal, false)? {
                result.push(file_index.to_string());
            }
        }

        // the result has to be deterministic
        result.sort();
        Ok(result)
    }

    async fn add_image_description(&self, uid: Uid) -> Result<(), Error> {
        let description_path = Index::get_uid_path(
            &self.root_dir,
            IMAGE_DIR_NAME,
            uid,
            Some("json"),
        )?;
        let image_path = Index::get_uid_path(
            &self.root_dir,
            IMAGE_DIR_NAME,
            uid,
            Some("png"),
        )?;
        let parent_path = parent(&image_path)?;

        if !exists(&parent_path) {
            create_dir_all(&parent_path)?;
        }

        let image_bytes = read_bytes(&image_path)?;
        let image_bytes = encode_base64(&image_bytes);

        if let Ok(j) = read_string(&description_path) {
            if serde_json::from_str::<Value>(&j).is_ok() {
                return Ok(());
            }

            else {
                remove_file(&description_path)?;
            }
        }

        let mut context = tera::Context::new();
        context.insert("image_type", "png");
        context.insert("image_bytes", &image_bytes);
        let pdl = self.get_prompt("describe_image")?;
        let Pdl { messages, schema } = parse_pdl(
            &pdl,
            &context,
            "/",  // TODO: `<|media|>` is not supported for this prompt
            true,
            true,
        )?;

        let request = Request {
            messages,
            model: self.get_model_by_name(&self.api_config.model)?,
            frequency_penalty: None,
            max_tokens: None,
            max_retry: self.api_config.max_retry,
            sleep_between_retries: self.api_config.sleep_between_retries,
            timeout: self.api_config.timeout,
            temperature: None,
            record_api_usage_at: self.api_config.dump_api_usage_at.clone().map(
                |path| RecordAt { path, id: String::from("describe_image") }
            ),
            dump_pdl_at: self.api_config.create_pdl_path("describe_image"),
            dump_json_at: self.api_config.dump_log_at.clone(),
            schema,
            schema_max_try: 3,
        };
        let result = request.send_and_validate::<ImageDescription>(ImageDescription::default()).await?;

        write_bytes(
            &description_path,
            &serde_json::to_vec_pretty(&result)?,
            WriteMode::Atomic,
        )?;

        Ok(())
    }

    pub fn run_tfidf(
        &self,
        keywords: Keywords,
        limit: usize,
    ) -> Result<Vec<TfidfResult<Uid>>, Error> {
        let mut tfidf_state = TfidfState::new(&keywords);

        // TODO: it must be configurable, or at least do much more experiment on this
        let ii_coeff = 20;

        if self.query_config.enable_ii && self.is_ii_built() {
            for chunk_uid in self.get_search_candidates(
                &tfidf_state.terms,
                limit * ii_coeff,
            )? {
                let processed_doc = self.get_tfidf_by_chunk_uid(chunk_uid)?;
                consume_processed_doc(
                    processed_doc,
                    &mut tfidf_state,
                )?;
            }
        }

        else {
            for tfidf_file in self.get_all_tfidf_files()? {
                let processed_doc = tfidf::load_from_file(&tfidf_file)?;
                consume_processed_doc(
                    processed_doc,
                    &mut tfidf_state,
                )?;
            }
        }

        Ok(tfidf_state.get_top(limit))
    }

    pub fn get_chunk_by_uid(&self, uid: Uid) -> Result<Chunk, Error> {
        let chunk_at = Index::get_uid_path(
            &self.root_dir,
            CHUNK_DIR_NAME,
            uid,
            Some("chunk"),
        )?;

        if exists(&chunk_at) {
            return Ok(chunk::load_from_file(&chunk_at)?);
        }

        Err(Error::NoSuchChunk(uid))
    }

    pub fn check_chunk_by_uid(&self, uid: Uid) -> bool {
        if let Ok(chunk_at) = Index::get_uid_path(
            &self.root_dir,
            CHUNK_DIR_NAME,
            uid,
            Some("chunk"),
        ) {
            exists(&chunk_at)
        }

        else {
            false
        }
    }

    pub fn get_tfidf_by_chunk_uid(
        &self,
        uid: Uid,
    ) -> Result<ProcessedDoc, Error> {
        let tfidf_at = Index::get_uid_path(
            &self.root_dir,
            CHUNK_DIR_NAME,
            uid,
            Some("tfidf"),
        )?;

        if exists(&tfidf_at) {
            return Ok(tfidf::load_from_file(&tfidf_at)?);
        }

        Err(Error::NoSuchChunk(uid))
    }

    pub fn get_tfidf_by_file_uid(
        &self,
        uid: Uid,
    ) -> Result<ProcessedDoc, Error> {
        let chunk_uids = self.get_chunks_of_file(uid)?;
        let mut result = ProcessedDoc::empty();

        for uid in chunk_uids.iter() {
            result.extend(&self.get_tfidf_by_chunk_uid(*uid)?);
        }

        result.uid = Some(uid);
        Ok(result)
    }

    // every path in index.json are relative path to root_dir
    fn get_rag_path(root_dir: &Path, rel_path: &Path) -> Result<Path, Error> {
        Ok(normalize(
            &join3(
                root_dir,
                &INDEX_DIR_NAME.to_string(),
                rel_path,
            )?,
        )?)
    }

    pub(crate) fn get_data_path(root_dir: &Path, rel_path: &Path) -> Result<Path, Error> {
        Ok(normalize(
            &join(
                root_dir,
                rel_path,
            )?,
        )?)
    }

    /// `{root_dir}/.ragit/{dir}/uid_prefix/uid_suffix(.{ext})?`
    pub(crate) fn get_uid_path(root_dir: &str, dir: &str, uid: Uid, ext: Option<&str>) -> Result<Path, Error> {
        let dir = join3(
            root_dir,
            INDEX_DIR_NAME,
            dir,
        )?;
        let uid_prefix = uid.get_prefix();
        let uid_suffix = uid.get_suffix();

        let mut result = join3(
            &dir,
            &uid_prefix,
            &uid_suffix,
        )?;

        if let Some(ext) = ext {
            result = set_extension(&result, ext)?;
        }

        Ok(result)
    }

    // root_dir/.ragit/ii/term_hash_prefix/term_hash_suffix
    fn get_ii_path(root_dir: &str, term_hash: String) -> Path {
        let ii_at = join3(
            root_dir,
            &INDEX_DIR_NAME,
            &II_DIR_NAME,
        ).unwrap();
        let term_hash_prefix = term_hash.get(0..2).unwrap().to_string();
        let term_hash_suffix = term_hash.get(2..).unwrap().to_string();

        join3(
            &ii_at,
            &term_hash_prefix,
            &term_hash_suffix,
        ).unwrap()
    }

    fn get_api_config_path(&self) -> Result<Path, Error> {
        Ok(Index::get_rag_path(
            &self.root_dir,
            &join(
                CONFIG_DIR_NAME,
                API_CONFIG_FILE_NAME,
            )?,
        )?)
    }

    fn get_build_config_path(&self) -> Result<Path, Error> {
        Ok(Index::get_rag_path(
            &self.root_dir,
            &join(
                CONFIG_DIR_NAME,
                BUILD_CONFIG_FILE_NAME,
            )?,
        )?)
    }

    fn get_query_config_path(&self) -> Result<Path, Error> {
        Ok(Index::get_rag_path(
            &self.root_dir,
            &join(
                CONFIG_DIR_NAME,
                QUERY_CONFIG_FILE_NAME,
            )?,
        )?)
    }

    pub(crate) fn init_api_config(&self, raw: &ApiConfigRaw) -> Result<ApiConfig, Error> {
        let dump_log_at = if raw.dump_log {
            let path = Index::get_rag_path(
                &self.root_dir,
                &LOG_DIR_NAME.to_string(),
            )?;

            if !exists(&path) || !is_dir(&path) {
                create_dir_all(&path)?;
            }

            Some(path)
        }

        else {
            None
        };

        let dump_api_usage_at = if raw.dump_api_usage {
            let path = Index::get_rag_path(
                &self.root_dir,
                &"usages.json".to_string(),
            )?;

            if !exists(&path) || is_dir(&path) {
                write_string(
                    &path,
                    "{}",
                    WriteMode::AlwaysCreate,
                )?;
            }

            Some(path)
        }

        else {
            None
        };

        Ok(ApiConfig {
            api_key: raw.api_key.clone(),
            max_retry: raw.max_retry,
            model: raw.model.clone(),
            timeout: raw.timeout,
            sleep_after_llm_call: raw.sleep_after_llm_call,
            sleep_between_retries: raw.sleep_between_retries,
            dump_log_at,
            dump_api_usage_at,
        })
    }

    // TODO: there must be an API function that modifies prompts
    pub(crate) fn load_prompts(&mut self) -> Result<(), Error> {
        for prompt_name in PROMPTS.keys() {
            let prompt_path = Index::get_rag_path(
                &self.root_dir,
                &join(
                    PROMPT_DIR,
                    &set_extension(
                        prompt_name,
                        "pdl",
                    )?,
                )?,
            )?;

            match read_string(&prompt_path) {
                Ok(p) => {
                    self.prompts.insert(prompt_name.to_string(), p);
                },
                Err(_) => {
                    eprintln!("Warning: failed to load `{prompt_name}.pdl`");
                },
            }
        }

        Ok(())
    }

    pub(crate) fn save_prompts(&self) -> Result<(), Error> {
        let prompt_real_dir = Index::get_rag_path(
            &self.root_dir,
            &PROMPT_DIR.to_string(),
        )?;

        if !exists(&prompt_real_dir) {
            create_dir_all(&prompt_real_dir)?;
        }

        for (prompt_name, prompt) in self.prompts.iter() {
            let prompt_path = join(
                &prompt_real_dir,
                &set_extension(
                    prompt_name,
                    "pdl",
                )?,
            )?;

            write_string(
                &prompt_path,
                prompt,
                WriteMode::Atomic,
            )?;
        }

        Ok(())
    }

    pub(crate) fn load_or_init_models(&mut self) -> Result<(), Error> {
        let models_at = Index::get_rag_path(
            &self.root_dir,
            &MODEL_FILE_NAME.to_string(),
        )?;

        if !exists(&models_at) {
            let default_models = ModelRaw::default_models();
            write_string(
                &models_at,
                &serde_json::to_string_pretty(&default_models)?,
                WriteMode::Atomic,
            )?;
        }

        let j = read_string(&models_at)?;
        let models = serde_json::from_str::<Vec<ModelRaw>>(&j)?;
        let mut result = vec![];

        for model in models.iter() {
            result.push(Model::try_from(model)?);
        }

        self.models = result;
        Ok(())
    }

    pub(crate) fn get_model_by_name(&self, name: &str) -> Result<Model, Error> {
        Ok(ragit_api::get_model_by_name(&self.models, name)?.clone())
    }

    pub fn get_prompt(&self, prompt_name: &str) -> Result<String, Error> {
        match self.prompts.get(prompt_name) {
            Some(prompt) => Ok(prompt.to_string()),
            None => Err(Error::PromptMissing(prompt_name.to_string())),
        }
    }

    fn add_file_index(&mut self, file_uid: Uid, uids: &[Uid]) -> Result<(), Error> {
        let file_index_path = Index::get_uid_path(
            &self.root_dir,
            FILE_INDEX_DIR_NAME,
            file_uid,
            None,
        )?;
        let parent_path = parent(&file_index_path)?;

        if !exists(&parent_path) {
            create_dir_all(&parent_path)?;
        }

        uid::save_to_file(&file_index_path, uids)
    }

    fn remove_file_index(&mut self, file_uid: Uid) -> Result<(), Error> {
        let file_index_path = Index::get_uid_path(
            &self.root_dir,
            FILE_INDEX_DIR_NAME,
            file_uid,
            None,
        )?;

        if !exists(&file_index_path) {
            return Err(Error::NoSuchFile { path: None, uid: Some(file_uid) });
        }

        Ok(remove_file(&file_index_path)?)
    }

    pub fn get_chunks_of_file(&self, file_uid: Uid) -> Result<Vec<Uid>, Error> {
        let file_index_path = Index::get_uid_path(
            &self.root_dir,
            FILE_INDEX_DIR_NAME,
            file_uid,
            None,
        )?;
        let mut result = vec![];

        if exists(&file_index_path) {
            for uid_str in read_string(&file_index_path)?.lines() {
                result.push(uid_str.parse::<Uid>()?);
            }

            return Ok(result);
        }

        Err(Error::NoSuchFile { path: None, uid: Some(file_uid) })
    }

    pub fn get_images_of_file(&self, file_uid: Uid) -> Result<Vec<Uid>, Error> {
        let chunk_uids = self.get_chunks_of_file(file_uid)?;
        let mut result = HashSet::new();

        for chunk_uid in chunk_uids.into_iter() {
            let chunk = self.get_chunk_by_uid(chunk_uid)?;

            for image in chunk.images.iter() {
                result.insert(*image);
            }
        }

        Ok(result.into_iter().collect())
    }

    pub fn load_image_by_uid(&self, uid: Uid) -> Result<Vec<u8>, Error> {
        Ok(read_bytes(&Index::get_uid_path(&self.root_dir, IMAGE_DIR_NAME, uid, Some("png"))?)?)
    }
}
