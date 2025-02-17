/*
- GET `/{user-name}/{repo-name}/index`
  - 200: application/json
    - index.json
  - 404
- GET `/{user-name}/{repo-name}/config/{config-name}`
  - 200: application/json
    - {config-name}.json
  - 404
- GET `/{user-name}/{repo-name}/prompt/{prompt-name}`
  - 200: text/plain
    - {prompt-name}.pdl
  - 404
- GET `/{user-name}/{repo-name}/chunk-count`
  - 200: application/json
    - an integer
  - 404, 500
- GET `/{user-name}/{repo-name}/chunk-list/{uid-prefix}`
  - 200: application/json
    - array[string]
- GET `/{user-name}/{repo-name}/chunk-list`
  - 200: application/json
    - array[string]
  - 404
- GET `/{user-name}/{repo-name}/chunk/{chunk-uid}`
  - 200: application/octet-stream
    - a chunk
    - you have to use `chunk::load_from_file()` to deserialize this data
  - 400, 404
- GET `/{user-name}/{repo-name}/archive-list`
  - 200: application/json
    - array[string]
  - 404
- GET `/{user-name}/{repo-name}/archive/{archive-key}`
  - 200: application/octet-stream
    - a file generated by `rag archive-create`
  - 404
- GET `/{user-name}/{repo-name}/image-list/{uid-prefix}`
  - 200: application/json
    - array[string]
- GET `/{user-name}/{repo-name}/image/{image-uid}`
  - 200: image/png
  - 400, 404
- GET `/{user-name}/{repo-name}/image-desc/{image-uid}`
  - 200: application/json
    - { extracted_text: string, explanation: string }
  - 400, 404
- GET `/{user-name}/{repo-name}/meta`
  - 200: application/json
  - 404
- GET `/{user-name}/{repo-name}/version`
  - 200: text/plain
    - "{major}.{minor}.{patch}"
    - "{major}.{minor}.{patch}-dev"
  - 404
- GET `/version`
  - 200: text/plain
    - "{major}.{minor}.{patch}"
    - "{major}.{minor}.{patch}-dev"
- POST `/{user-name}/{repo-name}/begin-push`
  - 200: text/plain
    - a session id
  - 404
- POST `/{user-name}/{repo-name}/archive`
  - body (multiform): { "session-id": str, "archive-id": str, "archive": bytes }
  - 200
  - 404
- POST `/{user-name}/{repo-name}/finalize-push`
  - body (plain text): session-id
  - 200
  - 404
*/

use crate::methods::*;
use ragit_fs::{
    initialize_log_file,
    set_log_file_path,
    write_log,
};
use warp::Filter;

mod methods;
mod utils;

#[tokio::main]
async fn main() {
    set_log_file_path(Some("ragit-server-logs".to_string()));
    initialize_log_file("ragit-server-logs", true).unwrap();

    let get_index_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("index"))
        .map(get_index);

    let get_config_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("config"))
        .and(warp::path::param::<String>())
        .map(get_config);

    let get_prompt_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("prompt"))
        .and(warp::path::param::<String>())
        .map(get_prompt);

    let get_chunk_count_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("chunk-count"))
        .map(get_chunk_count);

    let get_chunk_list_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("chunk-list"))
        .and(warp::path::param::<String>())
        .map(get_chunk_list);

    let get_chunk_list_all_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("chunk-list"))
        .and(warp::path::end())
        .map(get_chunk_list_all);

    let get_chunk_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("chunk"))
        .and(warp::path::param::<String>())
        .map(get_chunk);

    let get_image_list_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("image-list"))
        .and(warp::path::param::<String>())
        .map(get_image_list);

    let get_image_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("image"))
        .and(warp::path::param::<String>())
        .map(get_image);

    let get_image_desc_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("image-desc"))
        .and(warp::path::param::<String>())
        .map(get_image_desc);

    let get_archive_list_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("archive-list"))
        .map(get_archive_list);

    let get_archive_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("archive"))
        .and(warp::path::param::<String>())
        .map(get_archive);

    let get_meta_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("meta"))
        .map(get_meta);

    let get_version_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("version"))
        .map(get_version);

    let get_server_version_handler = warp::get()
        .and(warp::path("version"))
        .map(get_server_version);

    let post_begin_push_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("begin-push"))
        .map(post_begin_push);

    let post_archive_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("archive"))
        .map(post_archive);

    let post_finalize_push_handler = warp::get()
        .and(warp::path::param::<String>())
        .and(warp::path::param::<String>())
        .and(warp::path("finalize-push"))
        .map(post_finalize_push);

    let not_found_handler = warp::get().map(not_found);

    warp::serve(
        get_server_version_handler
            .or(get_index_handler)
            .or(get_config_handler)
            .or(get_prompt_handler)
            .or(get_chunk_count_handler)
            .or(get_chunk_list_handler)
            .or(get_chunk_list_all_handler)
            .or(get_chunk_handler)
            .or(get_image_list_handler)
            .or(get_image_handler)
            .or(get_image_desc_handler)
            .or(get_archive_list_handler)
            .or(get_archive_handler)
            .or(get_meta_handler)
            .or(get_version_handler)
            .or(post_begin_push_handler)
            .or(post_archive_handler)
            .or(post_finalize_push_handler)
            .or(not_found_handler)
            .with(warp::log::custom(
                |info| {
                    let headers = info.request_headers();

                    write_log(
                        &info.remote_addr().map(
                            |remote_addr| remote_addr.to_string()
                        ).unwrap_or_else(|| String::from("NO REMOTE ADDR")),
                        &format!(
                            "{:4} {:16} {:4} {headers:?}",
                            info.method().as_str(),
                            info.path(),
                            info.status().as_u16(),
                        ),
                    );
                }
            ))
    ).run(([0, 0, 0, 0], 41127)).await;  // TODO: configurable port number
}
