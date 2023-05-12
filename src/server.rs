use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{mpsc::Sender, Arc, Mutex, RwLock};
use std::thread;
use std::time::SystemTime;

use actix_files::file_extension_to_mime;
use actix_web::{
    dev::Server, get, http::header::CONTENT_TYPE, middleware, post, web, web::Bytes, App,
    HttpResponse, HttpServer, Responder,
};
use actix_web_static_files::{Resource, ResourceFiles};
use base64::engine::Engine;
use chrono::{offset::FixedOffset, NaiveDateTime, TimeZone};
use csv::{Error as CsvError, ReaderBuilder as CsvReaderBuilder};
use image::{io::Reader as ImageReader, ImageOutputFormat};
use mime::{Mime, IMAGE_JPEG, TEXT_HTML};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::{
    named_params, params, types::Value as SqlValue, Result as SqlResult, Statement, ToSql,
    Transaction,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_yaml;
use zip::ZipArchive;

const CONFIG_FILENAME: &str = "tmd-viewer.yaml";
const DATABASE_FILENAME: &str = "tmd-viewer.db";
const DEFAULT_DATA_DIR: &str = ".";
const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8888";
const DEFAULT_TIME_OFFSET_HOUR: f32 = 0.0f32; // UTC
const DEFAULT_SCANNER_COUNT_LIMIT: i32 = 2i32;
const DEFAULT_PAGE: i32 = 0i32;
const DEFAULT_PAGE_COUNT: i32 = 100i32;
const ONE_HOUR_I32: i32 = 3600i32;
const TWITTER_URL_REGEX: &str =
    r"^https?://(?:(?:mobile)\.)?twitter\.com/([a-zA-Z0-9_]+)/status/([0-9]+)";
// Default is 16. We are using probably more that.
// Feed queries = 2^5(num_where_clause) = 32
// + inserts + other queries
// https://github.com/rusqlite/rusqlite/blob/ddb7141c6dee4b8956af85b2e4a01a28e5fdbacc/src/lib.rs#L139
const STATEMENT_CACHE_SIZE: usize = 64usize;

struct AppState {
    config_path: RwLock<PathBuf>,
    data_dir: RwLock<String>,
    bind_address: RwLock<String>,
    pool: RwLock<Option<Pool<SqliteConnectionManager>>>,
    is_scanning: RwLock<bool>,
    scanner_count: RwLock<i32>,
    scanner_count_limit: i32,
    time_offset: f32,
}

#[derive(Serialize)]
struct AppStateExternal {
    data_dir: String,
    bind_address: String,
    time_offset: f32,
    is_scanning: bool,
    scanner_count: i32,
    scanner_count_limit: i32,
}

#[derive(Serialize)]
struct AppError {
    code: String,
    message: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct AppConfig {
    data_dir: Option<String>,
    bind_address: Option<String>,
    time_offset: Option<f32>,
    scanner_count_limit: Option<i32>,
}

#[derive(Deserialize)]
struct SetDataDirForm {
    data_dir: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FeedCsvRecord {
    feed_date: String,
    action_date: String,
    display_name: String,
    user_name: String,
    twitter_url: String,
    media_type: String,
    media_url: String,
    media_file_path: String,
    remarks: String,
    content: String,
    reply_count: String,
    retweet_count: String,
    like_count: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Media {
    #[serde(serialize_with = "format_string")]
    feed_id: i64,
    #[serde(serialize_with = "format_string")]
    media_id: i64,
    media_type: String,
    media_url: String,
    file_path: String,
    media_path: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_blob"
    )]
    thumbnail: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum FeedType {
    Feed {
        #[serde(serialize_with = "format_string")]
        feed_id: i64,
        feed_at: i64,
        user_name: String,
        twitter_url: String,
        contents: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        media: Option<Vec<Media>>,
    },
    Retweet {
        retweet_at: i64,
        user_name: String,
        #[serde(serialize_with = "format_string")]
        retweet_id: i64,
        retweet_user_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        retweet: Option<Box<FeedType>>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct FeedsResponse {
    query: FeedsQuery,
    feeds: Vec<FeedType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FeedsQuery {
    user_name: Option<String>,
    keyword: Option<String>,
    since: Option<String>,
    until: Option<String>,
    has_media_only: Option<bool>,
    page: Option<i32>,
    count: Option<i32>,
}

// https://github.com/serde-rs/serde/issues/661#issuecomment-269858463
// https://github.com/serde-rs/serde/issues/1059
fn serialize_blob<S: Serializer>(
    value: &Option<Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(&value.as_ref().unwrap_or(&Vec::new())),
    )
}

fn format_string<S: Serializer, V: core::fmt::Display>(
    value: V,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}", value))
}

// fn from_base64<D>(deserializer: &mut D) -> Result<Option<Vec<u8>>, D::Error> where D: Deserializer {
//     use serde::de::Error as SerdeError;
//     String::deserialize(deserializer)
//         .and_then(|string| base64::decode(&string).map_err(|err| Error::custom(err.to_string())))
//         .map(|bytes| Some(&bytes))
//         .and_then(|opt| opt.ok_or_else(|| SerdeError::custom("failed to deserialize blob")))
// }

fn str_to_timestamp(value: &str, offset: i32) -> Option<i64> {
    match NaiveDateTime::parse_from_str(value, "%Y/%m/%d %H:%M:%S") {
        Ok(dt) => {
            let dt = FixedOffset::east(offset).from_local_datetime(&dt).unwrap();
            Some(dt.timestamp())
        }
        Err(_err) => None,
    }
}

/// Services

fn state(data: web::Data<AppState>) -> AppStateExternal {
    AppStateExternal {
        data_dir: data.data_dir.read().unwrap().to_string(),
        bind_address: data.bind_address.read().unwrap().to_string(),
        time_offset: data.time_offset,
        is_scanning: *data.is_scanning.read().unwrap(),
        scanner_count: *data.scanner_count.read().unwrap(),
        scanner_count_limit: data.scanner_count_limit,
    }
}

// https://fullstackmilk.dev/efficiently_escaping_strings_using_cow_in_rust/
fn escape_like_char(ch: char) -> Option<&'static str> {
    match ch {
        '%' => Some("\\%"),
        _ => None,
    }
}

// https://fullstackmilk.dev/efficiently_escaping_strings_using_cow_in_rust/
fn escape_like_str(input: &str) -> Cow<str> {
    // Iterate through the characters, checking if each one needs escaping
    for (i, ch) in input.chars().enumerate() {
        if escape_like_char(ch).is_some() {
            // At least one char needs escaping, so we need to return a brand
            // new `String` rather than the original

            let mut escaped_string = String::with_capacity(input.len());
            // Calling `String::with_capacity()` instead of `String::new()` is
            // a slight optimisation to reduce the number of allocations we
            // need to do.
            //
            // We know that the escaped string is always at least as long as
            // the unescaped version so we can preallocate at least that much
            // space.

            // We already checked the characters up to index `i` don't need
            // escaping so we can just copy them straight in
            escaped_string.push_str(&input[..i]);

            // Escape the remaining characters if they need it and add them to
            // our escaped string
            for ch in input[i..].chars() {
                match escape_like_char(ch) {
                    Some(escaped_char) => escaped_string.push_str(escaped_char),
                    None => escaped_string.push(ch),
                };
            }

            return Cow::Owned(escaped_string);
        }
    }

    // We've iterated through all of `input` and didn't find any special
    // characters, so it's safe to just return the original string
    Cow::Borrowed(input)
}

fn get_feeds_query(query: &FeedsQuery) -> String {
    let mut where_clauses: Vec<&str> = Vec::new();
    if query.user_name.as_ref().is_some() && !query.user_name.as_ref().unwrap().is_empty() {
        where_clauses.push("f.user_name LIKE :user_name");
    }
    if query.keyword.as_ref().is_some() && !query.keyword.as_ref().unwrap().is_empty() {
        where_clauses.push("f.contents LIKE :keyword");
    }
    if query.has_media_only.as_ref().is_some() && *query.has_media_only.as_ref().unwrap() {
        where_clauses
            .push("EXISTS (SELECT m.feed_id FROM media m WHERE f.feed_id = m.feed_id LIMIT 1)");
    }
    let where_clause: String = if where_clauses.is_empty() {
        String::from("")
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };
    format!("SELECT \
    f.feed_id, f.feed_at, f.user_name, f.retweet_id, f.retweet_user_name, f.twitter_url, f.contents, \
    r.feed_id, r.feed_at, r.user_name, r.retweet_id, r.retweet_user_name, r.twitter_url, r.contents \
    FROM feeds f \
    LEFT JOIN feeds r \
    ON f.retweet_id = r.feed_id AND f.retweet_id != 0 \
    {where_clause} \
    ORDER BY f.feed_at DESC \
    LIMIT :limit OFFSET :offset", where_clause = where_clause)
}

fn fix_user_name(value: &Option<String>) -> Option<String> {
    match value {
        Some(s) => {
            if s.is_empty() {
                None
            } else {
                if s.starts_with("@") {
                    Some(s.to_owned())
                } else {
                    Some(format!("@{}", s.to_owned()))
                }
            }
        }
        None => None,
    }
}

#[get("/a/feeds")]
async fn feeds_service(
    web_query: web::Query<FeedsQuery>,
    data: web::Data<AppState>,
) -> impl Responder {
    let mut query = web_query.into_inner();
    query.user_name = fix_user_name(&query.user_name);
    query.page = Some(query.page.unwrap_or(DEFAULT_PAGE));
    query.count = Some(query.count.unwrap_or(DEFAULT_PAGE_COUNT));
    // println!("feeds: query: {:?}", &query);
    // println!("feeds: sql: {:?}", get_feeds_query(&query));
    // open_db(data.clone());
    // let conn = data.pool.read().unwrap().as_ref().unwrap().get().unwrap();
    // conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE_SIZE);
    let conn = get_conn(data.clone());
    let mut feeds_stmt = conn.prepare_cached(&get_feeds_query(&query)).unwrap();
    let mut feeds_params: Vec<(&str, &dyn ToSql)> = Vec::new();

    let page: i32 = query.page.unwrap();
    let count: i32 = query.count.unwrap();
    let offset = SqlValue::Integer(i64::from(page) * i64::from(count));
    let limit = SqlValue::Integer(i64::from(count));
    feeds_params.push((":offset", &offset));
    feeds_params.push((":limit", &limit));
    if query.user_name.is_some() {
        feeds_params.push((":user_name", &query.user_name));
    }
    let mut feeds_param_keyword = None;
    if query.keyword.is_some() && !query.keyword.as_ref().unwrap().is_empty() {
        let keyword_original = query.keyword.as_ref().unwrap();
        feeds_param_keyword = Some(format!(
            "%{}%",
            escape_like_str(&keyword_original).into_owned()
        ));
        feeds_params.push((":keyword", &feeds_param_keyword));
    }
    let mut feeds_result: SqlResult<Vec<FeedType>> = feeds_stmt
        .query_map(&feeds_params[..], |row| {
            let retweet_id: i64 = row.get(3).unwrap_or(0i64);
            if retweet_id == 0i64 {
                // Feed
                Ok(FeedType::Feed {
                    feed_id: row.get(0).unwrap(),
                    feed_at: row.get(1).unwrap(),
                    user_name: row.get(2).unwrap(),
                    twitter_url: row.get(5).unwrap(),
                    contents: row.get(6).unwrap(),
                    media: None,
                })
            } else {
                // Retweet
                let retweet_feed_id: i64 = row.get(7).unwrap_or(0i64);
                if retweet_feed_id == 0i64 {
                    // Retweet feed not in database
                    Ok(FeedType::Retweet {
                        retweet_at: row.get(1).unwrap(),
                        user_name: row.get(2).unwrap(),
                        retweet_id: row.get(3).unwrap(),
                        retweet_user_name: row.get(4).unwrap(),
                        retweet: None,
                    })
                } else {
                    // Retweet feed in database
                    Ok(FeedType::Retweet {
                        retweet_at: row.get(1).unwrap(),
                        user_name: row.get(2).unwrap(),
                        retweet_id: row.get(3).unwrap(),
                        retweet_user_name: row.get(4).unwrap(),
                        retweet: Some(Box::new(FeedType::Feed {
                            feed_id: row.get(7).unwrap(),
                            feed_at: row.get(8).unwrap(),
                            user_name: row.get(9).unwrap(),
                            twitter_url: row.get(12).unwrap(),
                            contents: row.get(13).unwrap(),
                            media: None,
                        })),
                    })
                }
            }
        })
        .and_then(Iterator::collect);
    let mut feeds = match feeds_result {
        Ok(arr) => arr,
        Err(err) => {
            println!("query error: {:?}", err);
            vec![]
        }
    };

    // Fill in media
    // TODO: change media without copying item
    for feed in feeds.iter_mut() {
        match feed {
            FeedType::Feed {
                feed_id,
                feed_at,
                user_name,
                twitter_url,
                contents,
                ..
            } => {
                // println!("----- {:?}", feed_id);
                *feed = FeedType::Feed {
                    feed_id: *feed_id,
                    feed_at: *feed_at,
                    user_name: user_name.clone(),
                    twitter_url: twitter_url.clone(),
                    contents: contents.clone(),
                    media: get_feed_media(&conn, *feed_id),
                };
            }
            FeedType::Retweet {
                retweet_at,
                user_name,
                retweet_id,
                retweet_user_name,
                retweet,
            } => match retweet {
                Some(retweet_feed) => {
                    // println!("----- RT {:?}", retweet_id);
                    let inner_feed: &FeedType = retweet_feed;
                    match inner_feed {
                        FeedType::Retweet { .. } => {}
                        FeedType::Feed {
                            feed_id: inner_feed_id,
                            feed_at: inner_feed_at,
                            user_name: inner_user_name,
                            twitter_url: inner_twitter_url,
                            contents: inner_contents,
                            ..
                        } => {
                            *feed = FeedType::Retweet {
                                retweet_at: *retweet_at,
                                user_name: user_name.clone(),
                                retweet_id: *retweet_id,
                                retweet_user_name: retweet_user_name.clone(),
                                retweet: Some(Box::new(FeedType::Feed {
                                    feed_id: *inner_feed_id,
                                    feed_at: *inner_feed_at,
                                    user_name: inner_user_name.clone(),
                                    twitter_url: inner_twitter_url.clone(),
                                    contents: inner_contents.clone(),
                                    media: get_feed_media(&conn, *inner_feed_id),
                                })),
                            };
                        }
                    };
                }
                None => {}
            },
        };
    }

    HttpResponse::Ok().json(FeedsResponse {
        query: query,
        feeds: feeds,
    })
}

fn get_feed_media(
    conn: &PooledConnection<SqliteConnectionManager>,
    media_feed_id: i64,
) -> Option<Vec<Media>> {
    let mut media_stmt = conn
        .prepare(
            "SELECT \
            feed_id, media_id, media_type, media_url, file_path, media_path, thumbnail, deleted_at \
            FROM media \
            WHERE feed_id = :media_feed_id",
        )
        .unwrap();
    let media_list: rusqlite::Result<Vec<Media>> = media_stmt
        .query_map(
            named_params! {
                ":media_feed_id": media_feed_id,
            },
            |row| {
                Ok(Media {
                    feed_id: row.get(0).unwrap(),
                    media_id: row.get(1).unwrap(),
                    media_type: row.get(2).unwrap(),
                    media_url: row.get(3).unwrap(),
                    file_path: row.get(4).unwrap(),
                    media_path: row.get(5).unwrap(),
                    thumbnail: match row.get(6) {
                        Ok(value) => Some(value),
                        Err(_err) => None,
                    },
                    deleted_at: row.get(7).unwrap(),
                })
            },
        )
        .and_then(Iterator::collect);
    match media_list {
        Ok(l) => {
            if l.is_empty() {
                None
            } else {
                Some(l)
            }
        }
        Err(_err) => None,
    }
}

#[get("/a/state")]
async fn app_state_service(data: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(state(data.clone()))
}

#[get("/")]
async fn home_service() -> impl Responder {
    return HttpResponse::Ok()
        .header(CONTENT_TYPE, TEXT_HTML)
        .body(Bytes::from_static(include_bytes!("../static/index.html")));
}

#[get("/a/media/file/{feed_id}/{media_id}")]
async fn media_file_service(
    web::Path((param_feed_id, param_media_id)): web::Path<(String, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    println!(
        "media_file_service {:?} {:?}",
        param_feed_id, param_media_id
    );
    let mut feed_id = 0i64;
    let mut media_id = 0i64;
    match param_feed_id.parse::<i64>() {
        Ok(v) => feed_id = v,
        Err(_err) => return HttpResponse::NotFound().body(""),
    };
    match param_media_id.parse::<i64>() {
        Ok(v) => media_id = v,
        Err(_err) => return HttpResponse::NotFound().body(""),
    };
    // println!("media_file_service {:?} {:?}", feed_id, media_id);

    let mut conn = get_conn(data.clone());
    let mut stmt = conn
        .prepare_cached(
            "SELECT \
            feed_id, media_id, media_type, media_url, file_path, media_path, thumbnail, deleted_at \
            FROM media \
            WHERE feed_id = :feed_id AND media_id = :media_id \
            LIMIT 1",
        )
        .unwrap();
    let mut media = None;
    match stmt.query_row(
        named_params! {
            ":feed_id": feed_id,
            ":media_id": media_id,
        },
        |row| {
            // println!("media_file_service fetched {:?} {:?}", feed_id, media_id);
            Ok(Media {
                feed_id: row.get(0).unwrap(),
                media_id: row.get(1).unwrap(),
                media_type: row.get(2).unwrap(),
                media_url: row.get(3).unwrap(),
                file_path: row.get(4).unwrap(),
                media_path: row.get(5).unwrap(),
                thumbnail: None,
                deleted_at: row.get(7).unwrap(),
            })
        },
    ) {
        Ok(value) => {
            // println!("media_file_service queried: {:?}", value);
            media = Some(value)
        }
        Err(_err) => return HttpResponse::NotFound().body(""),
    };
    match extract_zip_file(
        data.data_dir.read().unwrap().to_string(),
        media.as_ref().unwrap().file_path.to_string(),
        media.as_ref().unwrap().media_path.to_string(),
    ) {
        Some((buf, _size, mime_type)) => {
            return HttpResponse::Ok().header(CONTENT_TYPE, mime_type).body(buf);
        }
        None => {
            println!("media_file_service extract_zip_file failed");
        }
    }
    HttpResponse::NotFound().body("")
}

#[get("/a/media/preview/{feed_id}/{media_id}")]
async fn media_preview_service(
    web::Path((param_feed_id, param_media_id)): web::Path<(String, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    println!(
        "media_preview_service {:?} {:?}",
        param_feed_id, param_media_id
    );
    let mut feed_id = 0i64;
    let mut media_id = 0i64;
    match param_feed_id.parse::<i64>() {
        Ok(v) => feed_id = v,
        Err(_err) => return HttpResponse::NotFound().body(""),
    };
    match param_media_id.parse::<i64>() {
        Ok(v) => media_id = v,
        Err(_err) => return HttpResponse::NotFound().body(""),
    };

    let mut media = match get_media(data.clone(), feed_id, media_id) {
        Ok(value) => {
            if "Image" == &value.media_type && value.deleted_at.as_ref().is_none() {
                match value.thumbnail {
                    Some(buf) => {
                        return HttpResponse::Ok()
                            .header(CONTENT_TYPE, IMAGE_JPEG)
                            .body(buf);
                    }
                    None => value,
                }
            } else {
                return HttpResponse::NotFound().body("");
            }
        }
        Err(_err) => return HttpResponse::NotFound().body(""),
    };

    let mut image_blob = match extract_zip_file(
        data.data_dir.read().unwrap().to_string(),
        media.file_path.to_string(),
        media.media_path.to_string(),
    ) {
        Some((buf, _size, _mime_type)) => buf,
        None => return HttpResponse::NotFound().body(""),
    };

    match generate_thumbnail_blob(&image_blob) {
        Ok(buf) => media.thumbnail = Some(buf),
        Err(err) => println!("generate_thumbnail_blob update failed: {:?}", err),
    };

    match update_media_thumbnail(data.clone(), &media) {
        Ok(()) => {
            return HttpResponse::Ok()
                .header(CONTENT_TYPE, IMAGE_JPEG)
                .body(media.thumbnail.unwrap())
        }
        Err(err) => println!("update_media_thumbnail update failed: {:?}", err),
    };

    HttpResponse::NotFound().body("")
}

fn get_media(
    data: web::Data<AppState>,
    feed_id: i64,
    media_id: i64,
) -> Result<Media, rusqlite::Error> {
    let mut conn = get_conn(data.clone());
    let mut stmt = conn
        .prepare_cached(
            "SELECT \
            feed_id, media_id, media_type, media_url, file_path, media_path, thumbnail, deleted_at \
            FROM media \
            WHERE feed_id = :feed_id AND media_id = :media_id \
            LIMIT 1",
        )
        .unwrap();
    stmt.query_row(
        named_params! {
            ":feed_id": feed_id,
            ":media_id": media_id,
        },
        |row| {
            Ok(Media {
                feed_id: row.get(0).unwrap(),
                media_id: row.get(1).unwrap(),
                media_type: row.get(2).unwrap(),
                media_url: row.get(3).unwrap(),
                file_path: row.get(4).unwrap(),
                media_path: row.get(5).unwrap(),
                thumbnail: row.get(6).unwrap(),
                deleted_at: row.get(7).unwrap(),
            })
        },
    )
}

#[get("/a/zip/{zip_file_name}/{file_name:.*}")]
async fn zip_service(
    web::Path((zip_file_name, file_name)): web::Path<(String, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    println!("zip_service {} {}", zip_file_name, file_name);
    let zip_path = PathBuf::from(data.data_dir.read().unwrap().to_string()).join(zip_file_name);
    if zip_path.is_file() {
        let zip_file = File::open(zip_path).unwrap();
        match ZipArchive::new(zip_file) {
            Ok(mut zip) => match zip.by_name(&file_name) {
                Ok(mut f) => {
                    if f.is_file() {
                        let path = PathBuf::from(&file_name);
                        let ext = path.extension().unwrap_or(OsStr::new("")).to_str().unwrap();
                        let mut buf: Vec<u8> = Vec::new();
                        let _buf_size = f.read_to_end(&mut buf).unwrap();
                        return HttpResponse::Ok()
                            .header(CONTENT_TYPE, file_extension_to_mime(ext))
                            .body(buf);
                    }
                }
                Err(_err) => {}
            },
            Err(_err) => {}
        }
    }
    HttpResponse::NotFound().body("")
}

fn extract_zip_file(
    data_dir: String,
    zip_path: String,
    file_path: String,
) -> Option<(Vec<u8>, usize, Mime)> {
    let mut zip_path = PathBuf::from(&data_dir).join(&zip_path);
    if zip_path.is_file() {
        let zip_file = File::open(zip_path).unwrap();
        match ZipArchive::new(zip_file) {
            Ok(mut zip) => match zip.by_name(&file_path) {
                Ok(mut f) => {
                    if f.is_file() {
                        let path = PathBuf::from(&file_path);
                        let ext = path.extension().unwrap_or(OsStr::new("")).to_str().unwrap();
                        let mut buf: Vec<u8> = Vec::new();
                        let mut buf_size = f.read_to_end(&mut buf).unwrap();
                        return Some((buf, buf_size, file_extension_to_mime(ext)));
                    }
                }
                Err(_err) => {}
            },
            Err(_err) => {}
        }
    }
    None
}

#[post("/a/set_data_dir")]
async fn set_data_dir_service(
    (query, data): (web::Form<SetDataDirForm>, web::Data<AppState>),
) -> impl Responder {
    println!("/a/set_data_dir");
    let query = query.into_inner();
    let SetDataDirForm { data_dir } = query;
    match data_dir {
        Some(value) => {
            println!("data_dir={}", value);
            *data.data_dir.write().unwrap() = value.to_string();

            // Write config file
            let config = AppConfig {
                data_dir: Some(value.to_string()),
                bind_address: Some(data.bind_address.read().unwrap().clone()),
                time_offset: Some(data.time_offset),
                scanner_count_limit: Some(data.scanner_count_limit),
            };
            let config_str = serde_yaml::to_string(&config).unwrap();
            println!("write config: {:?}", config_str);
            let config_path = data.config_path.read().unwrap();
            fs::write(config_path.clone(), config_str).unwrap();

            HttpResponse::Ok().json(state(data.clone()))
        }
        None => HttpResponse::NotModified().body(""),
    }
}

#[post("/a/generate_thumbnails")]
async fn generate_thumbnails_service(data: web::Data<AppState>) -> impl Responder {
    if *data.scanner_count.read().unwrap() >= data.scanner_count_limit {
        return HttpResponse::TooManyRequests().json(state(data.clone()));
    }
    println!(
        "/a/generate_thumbnails start {} {:?}",
        data.scanner_count.read().unwrap(),
        data.data_dir.read().unwrap().to_string()
    );
    *data.scanner_count.write().unwrap() += 1;

    open_db(data.clone());

    // Generate all thumbnails
    generate_thumbnails(data.clone()).await;

    HttpResponse::Accepted().json(state(data.clone()))
}

async fn generate_thumbnails(data: web::Data<AppState>) {
    println!("generate_thumbnails");
    let thread_data = data.clone();
    thread::spawn(move || loop {
        let mut conn = get_conn(thread_data.clone());
        let mut pick_media_stmt = conn
            .prepare_cached(
                "SELECT \
                feed_id, media_id, media_type, media_url, file_path, media_path \
                FROM media \
                WHERE media_type = 'Image' \
                AND deleted_at IS NULL \
                AND thumbnail IS NULL \
                LIMIT 1",
            )
            .unwrap();
        let media = &mut match pick_media_stmt.query_row([], |row| {
            Ok(Media {
                feed_id: row.get(0).unwrap(),
                media_id: row.get(1).unwrap(),
                media_type: row.get(2).unwrap(),
                media_url: row.get(3).unwrap(),
                file_path: row.get(4).unwrap(),
                media_path: row.get(5).unwrap(),
                thumbnail: None,  // Filtered out
                deleted_at: None, // Filtered out
            })
        }) {
            Ok(media) => Some(media),
            Err(err) => {
                println!("generate_thumbnails failed picking file: {:?}", err);
                None
            }
        };

        match media {
            Some(value) => {
                generate_thumbnail(thread_data.clone(), value);
            }
            None => {
                *data.scanner_count.write().unwrap() -= 1;
                break;
            }
        };
    });
}

fn generate_thumbnail(data: web::Data<AppState>, media: &mut Media) {
    println!(
        "generate_thumbnail for {:?} {:?}",
        &media.feed_id, &media.media_id
    );
    let mut start = SystemTime::now();
    let zip_path =
        PathBuf::from(data.data_dir.read().as_ref().unwrap().to_string()).join(&media.file_path);
    if !zip_path.is_file() {
        println!("generate_thumbnail trying to read a non-file");
        match soft_delete_media_thumbnail(data.clone(), &media) {
            Ok(_) => {}
            Err(err) => {
                println!("soft_delete_media_thumbnail failed(0): {:?}", err);
            }
        };
        return;
    }
    let zip_file = File::open(zip_path).unwrap();
    let mut image_blob = Vec::new();
    match ZipArchive::new(zip_file) {
        Ok(mut zip) => match zip.by_name(&media.media_path) {
            Ok(mut f) => {
                if f.is_file() {
                    let _buf_size = f.read_to_end(&mut image_blob).unwrap();
                } else {
                    println!("generate_thumbnail trying to read a non-file");
                    match soft_delete_media_thumbnail(data.clone(), &media) {
                        Ok(_) => {}
                        Err(err) => {
                            println!("soft_delete_media_thumbnail failed(1): {:?}", err);
                        }
                    };
                    return;
                }
            }
            Err(err) => {
                println!("generate_thumbnail read failed: {:?}", err);
                match soft_delete_media_thumbnail(data.clone(), &media) {
                    Ok(_) => {}
                    Err(err) => {
                        println!("soft_delete_media_thumbnail failed(2): {:?}", err);
                    }
                };
                return;
            }
        },
        Err(err) => {
            println!("generate_thumbnail read failed: {:?}", err);
            match soft_delete_media_thumbnail(data.clone(), &media) {
                Ok(_) => {}
                Err(err) => {
                    println!("soft_delete_media_thumbnail failed(3): {:?}", err);
                    return;
                }
            };
            return;
        }
    };

    match generate_thumbnail_blob(&image_blob) {
        Ok(buf) => {
            media.thumbnail = Some(buf);
        }
        Err(err) => {
            println!("generate_thumbnail_blob update failed: {:?}", err);
        }
    };

    match update_media_thumbnail(data.clone(), &media) {
        Ok(()) => {}
        Err(err) => {
            println!("update_media_thumbnail update failed: {:?}", err);
        }
    };
}

fn generate_thumbnail_blob(blob: &Vec<u8>) -> Result<Vec<u8>, image::ImageError> {
    let last_time = SystemTime::now();

    let img_reader = ImageReader::new(Cursor::new(blob))
        .with_guessed_format()
        .expect("std::io::Cursor never fails");
    let mut img = img_reader.decode().unwrap();

    // image.thumbnail average 9sec!
    img = img.thumbnail(128u32, 128u32);

    // crop and resize
    // let cropped_size = std::cmp::min(img.width(), img.height());
    // if img.width() >= img.height() {
    //     let offset = img.width() - cropped_size / 2;
    //     img = img.crop(offset, offset + cropped_size, 0u32, cropped_size);
    // } else {
    //     let offset = img.height() - cropped_size / 2;
    //     img = img.crop(0u32, cropped_size, offset, offset + cropped_size);
    // }
    // img = img.resize(128u32, 128u32, image::imageops::FilterType::Lanczos3);

    let mut img_blob: Vec<u8> = Vec::new();
    match img.write_to(
        &mut Cursor::new(&mut img_blob),
        ImageOutputFormat::Jpeg(85u8),
    ) {
        Ok(_) => {
            let last_time_duration = SystemTime::now()
                .duration_since(last_time)
                .unwrap()
                .as_millis();
            // println!(
            //     "generate_thumbnail_bytes took {:?} [ms]",
            //     last_time_duration
            // );
            Ok(img_blob)
        }
        Err(err) => Err(err),
    }
}

fn update_media_thumbnail(data: web::Data<AppState>, media: &Media) -> Result<(), rusqlite::Error> {
    let conn = &mut get_conn(data.clone());
    let txn = conn.transaction().unwrap();
    {
        let update_thumbnail_stmt = &mut txn
            .prepare_cached(
                "UPDATE media SET thumbnail = :thumbnail \
                WHERE feed_id = :feed_id AND media_id = :media_id",
            )
            .unwrap();
        match update_thumbnail_stmt.execute(named_params! {
            ":feed_id":  media.feed_id,
            ":media_id":  media.media_id,
            ":thumbnail":  media.thumbnail,
        }) {
            Ok(_row_count) => {
                // println!(
                //     "update_media_thumbnail update success for: {:?} {:?}",
                //     media.feed_id, media.media_id
                // );
            }
            Err(err) => {
                println!(
                    "update_media_thumbnail update failed for: {:?} {:?}",
                    media.feed_id, media.media_id
                );
                return Err(err);
            }
        }
    }
    match txn.commit() {
        Ok(_) => {
            // println!(
            //     "update_media_thumbnail committed for: {:?} {:?}",
            //     media.feed_id, media.media_id
            // );
            Ok(())
        }
        Err(err) => {
            println!(
                "update_media_thumbnail commit failed for: {:?} {:?}",
                media.feed_id, media.media_id
            );
            Err(err)
        }
    }
}

fn soft_delete_media_thumbnail(
    data: web::Data<AppState>,
    media: &Media,
) -> Result<(), rusqlite::Error> {
    println!(
        "soft_delete_media_thumbnail {:?} {:?}",
        media.feed_id, media.media_id
    );
    let conn = &mut get_conn(data.clone());
    let txn = conn.transaction().unwrap();
    {
        let soft_delete_thumbnail_stmt = &mut txn
            .prepare_cached(
                "UPDATE media SET deleted_at = CAST(strftime('%s','now') AS INTEGER) \
                WHERE feed_id = :feed_id AND media_id = :media_id",
            )
            .unwrap();
        match soft_delete_thumbnail_stmt.execute(named_params! {
            ":feed_id":  media.feed_id,
            ":media_id":  media.media_id,
        }) {
            Ok(_row_count) => {
                // println!(
                //     "soft_delete_media_thumbnail update success for: {:?} {:?}",
                //     media.feed_id, media.media_id
                // );
            }
            Err(err) => {
                println!(
                    "soft_delete_media_thumbnail update failed for: {:?} {:?}",
                    media.feed_id, media.media_id
                );
                return Err(err);
            }
        };
    }
    match txn.commit() {
        Ok(_) => {
            // println!(
            //     "soft_delete_media_thumbnail committed for: {:?} {:?}",
            //     media.feed_id, media.media_id
            // );
            Ok(())
        }
        Err(err) => {
            println!(
                "soft_delete_media_thumbnail commit failed for: {:?} {:?}",
                media.feed_id, media.media_id
            );
            Err(err)
        }
    }
}

#[post("/a/clean")]
async fn clean_service(data: web::Data<AppState>) -> impl Responder {
    println!("clean_service");
    if *data.scanner_count.read().unwrap() > 0
        || (data.pool.read().unwrap().as_ref().is_some()
            && data
                .pool
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .state()
                .connections
                != data
                    .pool
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .state()
                    .idle_connections)
    {
        return HttpResponse::ServiceUnavailable().json(AppError {
            code: String::from("clean_service_01"),
            message: String::from("Database is in use"),
        });
    }

    open_db(data.clone());

    let conn = data.pool.read().unwrap().as_ref().unwrap().get().unwrap();
    conn.execute("DELETE FROM media;", []).unwrap();
    conn.execute("DELETE FROM feeds;", []).unwrap();
    conn.execute("DELETE FROM files;", []).unwrap();
    conn.execute("VACUUM;", []).unwrap();

    HttpResponse::Ok().json(state(data.clone()))
}

#[post("/a/scan")]
async fn scan_service(data: web::Data<AppState>) -> impl Responder {
    if *data.scanner_count.read().unwrap() >= data.scanner_count_limit {
        return HttpResponse::TooManyRequests().json(state(data.clone()));
    }
    println!(
        "/a/scan start {} {:?}",
        data.scanner_count.read().unwrap(),
        data.data_dir.read().unwrap().to_string()
    );
    *data.scanner_count.write().unwrap() += 1;

    open_db(data.clone());

    // List all zip
    list_all_zip(data.clone());

    // Scan oldest unscanned file until all are scanned
    scan_files(data.clone()).await;

    HttpResponse::Accepted().json(state(data.clone()))
}

async fn scan_files(data: web::Data<AppState>) {
    println!("scan_files");
    let thread_data = data.clone();
    thread::spawn(move || loop {
        let conn = get_conn(thread_data.clone());
        let mut pick_file_stmt = conn
            .prepare_cached("SELECT file_path FROM files WHERE scan_started_at IS NULL LIMIT 1")
            .unwrap();
        let mut file_name: Option<String> = None;
        match pick_file_stmt.query_row(&[] as &[&dyn rusqlite::types::ToSql], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(f) => file_name = Some(f),
            Err(err) => println!("scan_files failed picking file: {:?}", err),
        };
        let mut start_scan_stmt = conn.prepare_cached("UPDATE files SET scan_started_at = CAST(strftime('%s','now') AS INTEGER) WHERE file_path = $1").unwrap();
        let mut end_scan_stmt = conn.prepare_cached("UPDATE files SET scan_ended_at = CAST(strftime('%s','now') AS INTEGER) WHERE file_path = $1").unwrap();
        match file_name {
            Some(value) => {
                match start_scan_stmt.execute(&[&value]) {
                    Ok(_row_count) => println!("scan_files set scan_started_at"),
                    Err(err) => println!("scan_files set scan_started_at failed: {:?}", err),
                };
                scan_file(data.clone(), value.clone());
                match end_scan_stmt.execute(&[&value]) {
                    Ok(_row_count) => println!("scan_files set scan_ended_at"),
                    Err(err) => println!("scan_files set scan_ended_at failed: {:?}", err),
                };
            }
            None => {
                *data.scanner_count.write().unwrap() -= 1;
                break;
            }
        };
    });
}

fn scan_file(data: web::Data<AppState>, file_name: String) {
    println!("scan_file {:?}", file_name);
    let conn = &mut get_conn(data.clone());
    let zip_file_name = file_name.clone();
    let zip_path = PathBuf::from(data.data_dir.read().unwrap().to_string()).join(file_name);
    let zip_file = File::open(zip_path).unwrap();
    let mut zip = ZipArchive::new(zip_file).unwrap();
    for zip_index in 0..zip.len() {
        let file = zip.by_index(zip_index).unwrap();
        if file.is_file() {
            let path = PathBuf::from(file.enclosed_name().unwrap());
            if "csv".eq(path.extension().unwrap_or(OsStr::new(""))) {
                println!("scan_file {:?}", file.enclosed_name().unwrap());
                let csv = &mut CsvReaderBuilder::new().has_headers(false).from_reader(file);
                let txn = conn.transaction().unwrap();
                let record_count = process_csv(data.clone(), &txn, csv, zip_file_name.clone());
                match txn.commit() {
                    Ok(_) => println!("process_csv returned records: {:?}", record_count),
                    Err(err) => println!("process_csv commit errir: {:?}", err),
                };
            }
        }
    }
}

fn process_csv(
    data: web::Data<AppState>,
    txn: &Transaction<'_>,
    csv: &mut csv::Reader<zip::read::ZipFile>,
    zip_file_name: String,
) -> usize {
    let mut origin = String::from("");
    let mut record_count = 0usize;
    let mut insert_feed_stmt = &mut txn
        .prepare_cached(
            "INSERT OR IGNORE INTO feeds \
            (feed_id, user_name, retweet_id, retweet_user_name, feed_at, twitter_url, contents) \
            VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .unwrap();
    let mut insert_retweet_stmt = &mut txn
        .prepare_cached(
            "INSERT OR IGNORE INTO feeds \
            (feed_id, user_name, retweet_id, retweet_user_name, feed_at, twitter_url) \
            VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .unwrap();
    // let mut insert_media_stmt = &mut txn
    //     .prepare_cached(
    //         "INSERT OR IGNORE INTO media \
    //         (feed_id, media_url, media_type, file_path, media_path) \
    //         VALUES ($1, $2, $3, $4, $5)",
    //     )
    //     .unwrap();
    let mut insert_media_stmt = &mut txn
        .prepare_cached(
            "INSERT OR IGNORE INTO media \
                (feed_id, media_id, media_type, media_url, file_path, media_path) WITH \
                media_row AS ( \
                    SELECT \
                    feed_id, \
                    ROW_NUMBER() OVER (ORDER BY media_id DESC) AS next_media_id \
                    FROM media \
                    WHERE feed_id = :feed_id \
                    ORDER BY next_media_id DESC LIMIT 1 \
                ), \
                vals AS ( \
                    SELECT \
                    :feed_id AS feed_id, \
                    :media_type AS media_type, \
                    :media_url AS media_url, \
                    :file_path AS file_path, \
                    :media_path as media_path \
                ) \
                SELECT  \
                    v.feed_id AS feed_id,  \
                    IFNULL(r.next_media_id, 0) + 1 AS media_id,  \
                    v.media_type AS media_type, \
                    v.media_url AS media_url, \
                    v.file_path AS file_path, \
                    v.media_path AS media_path \
                FROM vals v \
                LEFT JOIN media_row r \
                ON v.feed_id = r.feed_id",
        )
        .unwrap();

    // NOTE: This may set off Inf or NaN which is why
    // data.time_offset must be sanitized on config read
    let time_offset_ms: i32 = data.time_offset.round() as i32 * ONE_HOUR_I32;
    for record in csv.deserialize() {
        record_count = record_count + 1;
        match record {
            Ok::<FeedCsvRecord, CsvError>(rec) => {
                if record_count == 3 {
                    origin = rec.action_date.to_ascii_lowercase().clone();
                }
                if record_count > 6 {
                    process_csv_record(
                        &mut insert_feed_stmt,
                        &mut insert_retweet_stmt,
                        &mut insert_media_stmt,
                        rec,
                        origin.clone(),
                        zip_file_name.clone(),
                        time_offset_ms,
                    );
                }
            }
            Err(err) => {
                println!("ERROR  {:?}", err);
            }
        }
    }
    println!("process_csv inserted {:?} rows", record_count);
    record_count
}

fn process_csv_record(
    // conn: &mut PooledConnection<SqliteConnectionManager>,
    insert_feed_stmt: &mut Statement<'_>,
    insert_retweet_stmt: &mut Statement<'_>,
    insert_media_stmt: &mut Statement<'_>,
    record: FeedCsvRecord,
    origin: String,
    zip_path: String,
    time_offset_ms: i32,
) {
    // println!("process_csv_record for {}", origin);
    let url_re = Regex::new(TWITTER_URL_REGEX).unwrap();
    let url_cap = url_re.captures(&record.twitter_url);
    let feed_id = match url_cap {
        Some(cap) => {
            // println!("  captured {:?}", cap);
            if cap.len() >= 2 {
                match cap[2].parse::<i64>() {
                    Ok(num) => Some(num),
                    Err(_err) => None,
                }
            } else {
                None
            }
        }
        None => None,
    };
    // println!("  twitter_url {:?}", record.twitter_url);
    // println!("  feed_id {:?}", feed_id);
    match feed_id {
        Some(id) => {
            let feed_at = str_to_timestamp(&record.feed_date, time_offset_ms);
            let action_at = str_to_timestamp(&record.action_date, time_offset_ms);
            if action_at.is_some() {
                // Insert retweet feed
                insert_feed(
                    insert_feed_stmt,
                    id,
                    record.user_name.to_ascii_lowercase(),
                    feed_at.unwrap(),
                    record.twitter_url.clone(),
                    record.content.clone(),
                );
                insert_retweet(
                    insert_retweet_stmt,
                    id,
                    record.user_name.to_ascii_lowercase(),
                    origin.clone(),
                    action_at.unwrap(),
                    record.twitter_url.clone(),
                );
            } else {
                // Insert feed
                insert_feed(
                    insert_feed_stmt,
                    id,
                    record.user_name.to_ascii_lowercase(),
                    feed_at.unwrap(),
                    record.twitter_url.clone(),
                    record.content.clone(),
                );
            }
            if !record.media_url.is_empty() && !record.media_file_path.is_empty() {
                // Insert media
                insert_media(
                    insert_media_stmt,
                    id,
                    record.media_type.clone(),
                    record.media_url.clone(),
                    zip_path.clone(),
                    record.media_file_path.clone(),
                );
            }
        }
        None => {}
    }
}

fn insert_feed(
    stmt: &mut Statement<'_>,
    feed_id: i64,
    user_name: String,
    feed_at: i64,
    twitter_url: String,
    contents: String,
) {
    match stmt.execute(params![
        feed_id,
        user_name,
        0i32,
        "",
        feed_at,
        twitter_url,
        contents
    ]) {
        Ok(count) => {
            if count > 0 {
                // println!("insert_feed: {:?} {:?}", user_name, feed_id);
            }
        }
        Err(err) => {
            println!("insert_feed error: {:?}", err);
        }
    };
}

fn insert_retweet(
    stmt: &mut Statement<'_>,
    retweet_id: i64,
    retweet_user_name: String,
    user_name: String,
    retweet_at: i64,
    twitter_url: String,
) {
    match stmt.execute(params![
        0i32,
        user_name,
        retweet_id,
        retweet_user_name,
        retweet_at,
        twitter_url
    ]) {
        Ok(count) => {
            if count > 0 {
                // println!("insert_retweet: {:?} {:?}", user_name, retweet_id);
            }
        }
        Err(err) => {
            println!("insert_retweet error: {:?}", err);
        }
    };
}

fn insert_media(
    stmt: &mut Statement,
    feed_id: i64,
    media_type: String,
    media_url: String,
    file_path: String,
    media_path: String,
) {
    match stmt.execute(named_params! {
        ":feed_id": feed_id,
        ":media_type": media_type,
        ":media_url": media_url,
        ":file_path": file_path,
        ":media_path": media_path
    }) {
        Ok(count) => {
            if count > 0 {
                // println!("insert_media: {:?} {:?}", feed_id, media_url);
            } else {
                println!("insert_media exists: {:?} {:?}", feed_id, media_url);
            }
        }
        Err(err) => {
            println!("insert_media error: {:?}", err);
        }
    };
}

fn list_all_zip(data: web::Data<AppState>) {
    println!("list_all_zip");
    let mut insert_count: usize = 0;
    // let conn = data.pool.read().unwrap().as_ref().unwrap().get().unwrap();
    let mut conn = get_conn(data.clone());
    let mut txn = conn.transaction().unwrap();
    fs::read_dir(PathBuf::from(data.data_dir.read().unwrap().to_string()))
        .unwrap()
        .into_iter()
        .map(|x| x.unwrap().path())
        .filter(|x| x.is_file() && "zip".eq(x.extension().unwrap_or(OsStr::new(""))))
        .for_each(|x| {
            let mut stmt = txn
                .prepare_cached("INSERT OR IGNORE INTO files (file_path) VALUES ($1)")
                .unwrap();
            // println!("scanning {:?}", x);
            match stmt.execute(params![x.file_name().unwrap().to_str()]) {
                Ok(count) => {
                    if count > 0 {
                        println!(
                            "list_all_zip inserted new file: {}",
                            x.file_name().unwrap().to_str().unwrap()
                        );
                    }
                    insert_count += count;
                }
                Err(err) => {
                    println!("list_all_zip update failed: {}", err);
                }
            };
        });
    match txn.commit() {
        Ok(_) => {
            println!("list_all_zip added {} new files", insert_count);
        }
        Err(err) => {
            println!("list_all_zip error: {:?}", err);
        }
    }
}

fn open_db(data: web::Data<AppState>) {
    // println!("open_db");
    let mut pool = None;
    if data.pool.read().unwrap().as_ref().is_none() {
        pool = Some(init_pool(PathBuf::from(data.data_dir.read().unwrap().to_string())).unwrap());
    };
    if pool.is_some() {
        *data.pool.write().unwrap() = pool;
    }
}

fn init_pool(data_dir: PathBuf) -> Option<Pool<SqliteConnectionManager>> {
    println!("init_pool");
    let data_file = data_dir.join(DATABASE_FILENAME);
    if data_file.exists()
        && (!data_file.metadata().unwrap().is_file()
            || data_file.metadata().unwrap().permissions().readonly())
    {
        println!("init_pool failed, file not accessible");
        return None;
    }

    let manager = SqliteConnectionManager::file(data_file);
    let pool = Pool::new(manager).unwrap();
    let conn = pool.get().unwrap();

    let create_tbl_data_files_sql = include_str!("create_table_files.sql");
    let create_tbl_feeds_sql = include_str!("create_table_feeds.sql");
    let create_tbl_media_sql = include_str!("create_table_media.sql");

    conn.execute(create_tbl_data_files_sql, []).unwrap();
    conn.execute(create_tbl_feeds_sql, []).unwrap();
    conn.execute(create_tbl_media_sql, []).unwrap();

    let create_idx_feeds_ids_sql = include_str!("create_index_feeds_ids.sql");
    let create_idx_feeds_ids_un_sql = include_str!("create_index_feeds_ids_un.sql");
    let create_idx_feeds_feed_at_sql = include_str!("create_index_feeds_feeds_at.sql");
    let create_idx_media_feed_id_sql = include_str!("create_index_media_feed_id.sql");
    let create_idx_media_ids_sql = include_str!("create_index_media_ids.sql");
    let create_idx_media_unique_sql = include_str!("create_index_media_unique.sql");

    conn.execute(create_idx_feeds_ids_sql, []).unwrap();
    conn.execute(create_idx_feeds_ids_un_sql, []).unwrap();
    conn.execute(create_idx_feeds_feed_at_sql, []).unwrap();
    conn.execute(create_idx_media_feed_id_sql, []).unwrap();
    conn.execute(create_idx_media_ids_sql, []).unwrap();
    conn.execute(create_idx_media_unique_sql, []).unwrap();

    println!("init_pool return pool");
    Some(pool)
}

fn get_conn(data: web::Data<AppState>) -> PooledConnection<SqliteConnectionManager> {
    open_db(data.clone());
    let conn: PooledConnection<SqliteConnectionManager> =
        data.pool.read().unwrap().as_ref().unwrap().get().unwrap();
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE_SIZE);
    conn
}

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

// https://docs.rs/actix-web/4.0.1/actix_web/rt/index.html
#[actix_web::main]
pub async fn serve(cwd: Box<String>, server_tx: Arc<Mutex<Sender<Server>>>) -> std::io::Result<()> {
    println!("current_dir: {:?}", std::env::current_dir());
    println!("current_exe: {:?}", std::env::current_exe());

    // current_dir is C:\Windows\System32 for windows service
    // Change current_dir to directory containing exe if run as service
    // http://haacked.com/archive/2004/06/29/current-directory-for-windows-service-is-not-what-you-expect.aspx/#:~:text=At%20least%20it%20wasn't,service%20is%20the%20System32%20folder.
    std::env::set_current_dir(*cwd).unwrap();

    let mut data_dir = DEFAULT_DATA_DIR.to_string();
    let mut bind_address = DEFAULT_BIND_ADDRESS.to_string();
    let mut time_offset = DEFAULT_TIME_OFFSET_HOUR;
    let mut scanner_count_limit = DEFAULT_SCANNER_COUNT_LIMIT;

    // Read config file if exists
    let config_path = std::env::current_dir().unwrap().join(CONFIG_FILENAME);
    println!("config_path: {:?}", config_path);
    // TODO: find tmd-viewer.yaml on current_dir -> current_exe.parent
    if config_path.exists() && config_path.metadata().unwrap().is_file() {
        let config_str = fs::read_to_string(&config_path).expect("File unreadable");
        println!("read config");
        let config: &AppConfig = &mut serde_yaml::from_str(&config_str).unwrap();
        data_dir = config.data_dir.as_ref().unwrap_or(&data_dir).clone();
        bind_address = config
            .bind_address
            .as_ref()
            .unwrap_or(&bind_address)
            .clone();
        scanner_count_limit = config.scanner_count_limit.unwrap_or(scanner_count_limit);
        let time_offset_hour = config.time_offset.unwrap_or(DEFAULT_TIME_OFFSET_HOUR);
        if time_offset_hour < -24f32 || time_offset_hour > 24f32 {
            panic!("time_offset out of range {:?}", config.time_offset.unwrap());
        } else {
            time_offset = time_offset_hour;
        }
    } else if !config_path.exists() {
        // Write config file with defaults
        let config = AppConfig {
            data_dir: Some(data_dir.to_string()),
            bind_address: Some(bind_address.to_string()),
            time_offset: Some(time_offset),
            scanner_count_limit: Some(scanner_count_limit),
        };
        let config_str = serde_yaml::to_string(&config).unwrap();
        println!("write config");
        fs::write(&config_path, config_str).unwrap();
    }

    // App-wide state
    let app_state = web::Data::new(AppState {
        config_path: RwLock::new(config_path),
        data_dir: RwLock::new(data_dir.to_string()),
        bind_address: RwLock::new(bind_address.to_string()),
        pool: RwLock::new(None),
        is_scanning: RwLock::new(false),
        scanner_count: RwLock::new(0),
        scanner_count_limit: scanner_count_limit, // readonly
        time_offset: time_offset,                 // readonly
    });

    // Start HTTP server
    println!("starting http://{}", bind_address);

    let server = HttpServer::new(move || {
        let static_files: HashMap<&'static str, Resource> = generate();

        App::new()
            .app_data(web::Data::clone(&app_state))
            .wrap(middleware::Compress::default())
            .service(ResourceFiles::new("/static", static_files))
            .service(feeds_service)
            .service(media_file_service)
            .service(media_preview_service)
            .service(zip_service)
            .service(app_state_service)
            .service(generate_thumbnails_service)
            .service(scan_service)
            .service(clean_service)
            .service(set_data_dir_service)
            .service(home_service)
    })
    .client_timeout(10000u64)
    .bind(bind_address)
    .unwrap()
    .run();

    server_tx.lock().unwrap().send(server.clone()).unwrap();
    server.await
}
