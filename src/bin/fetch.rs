use metrics::asana::*;
use metrics::config::*;

use clap::{App, Arg};
use env_logger;
use futures::future::{join, join3, join_all};
use serde_json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tokio;

fn main() {
    /* Logging */
    env_logger::init();

    /* Command Line */
    let (config_file_str, token_file_str) = process_command_line();

    /*
     * Config data
     */
    let config_file_path = Path::new(&config_file_str)
        .canonicalize()
        .expect(&format!("Bad config file path: {}", &config_file_str));
    let config_str = fs::read_to_string(config_file_path)
        .expect(&format!("Bad config file: {}", &config_file_str));
    let config: MyConfig = parse_config(&config_str);

    /*
     * Asana Personal Access Token -- credentials
     */
    let token_file_path: PathBuf = Path::new(&token_file_str)
        .canonicalize()
        .expect(&format!("Bad token file path: {}", &token_file_str));
    let token_str =
        fs::read_to_string(token_file_path).expect(&format!("Bad token file: {}", &token_file_str));
    let token_str = String::from(token_str.trim_end());
    /*
     * Process
     */
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(get_data(&token_str, &config));
}

fn process_command_line() -> (String, String) {
    let matches = App::new("fetch")
        .version("0.1.0")
        .author("Parijat Mishra <parijat.mishra@gmail.com>")
        .about("Scrape Asana for Kanban Metrics, write to output.json")
        .arg(
            Arg::with_name("config-file")
                .short("c")
                .long("config-file")
                .takes_value(true)
                .help("path to config file"),
        )
        .arg(
            Arg::with_name("token-file")
                .short("t")
                .long("token-file")
                .takes_value(true)
                .help("path of file containing an Asana Personal Access Token"),
        )
        .arg(
            Arg::with_name("output-file")
                .short("o")
                .long("output-file")
                .takes_value(true)
                .help("Output file (JSON data)"),
        )
        .get_matches();
    let config_file_str = matches
        .value_of("config-file")
        .expect("config-file must be specified");
    let token_file_str = matches
        .value_of("token-file")
        .expect("token-file must be specified");
    return (config_file_str.to_owned(), token_file_str.to_owned());
}

pub async fn get_data(token: &str, config: &MyConfig) {
    let client = AsanaClient::new(token, Some(2));

    let (asana_projects, asana_project_sections, asana_project_task_gids) =
        get_asana_data_projects(&client, config).await;

    let task_gids: Vec<_> = asana_project_task_gids
        .iter()
        .flat_map(|e| &e.task_gids)
        .collect();

    let (asana_tasks, asana_task_stories) = get_asana_data_tasks(&client, &task_gids).await;

    let user_gids: HashSet<_> = asana_tasks
        .iter()
        .filter(|&t| t.assignee.is_some())
        .map(|t| &t.assignee.as_ref().unwrap().gid)
        .collect();

    let asana_users = get_asana_data_users(&client, &user_gids).await;

    let data = AsanaData {
        users: asana_users,
        projects: asana_projects,
        project_sections: asana_project_sections,
        project_task_gids: asana_project_task_gids,
        tasks: asana_tasks,
        task_stories: asana_task_stories,
    };
    let output_filename = "asana_data.json";
    let output_str = serde_json::to_string(&data).expect("Should convert to JSON string");
    fs::write(output_filename, output_str).expect("Should write to file");

    println!("Wrote output to file {}.", output_filename);
}

async fn get_asana_data_projects(
    client: &AsanaClient<'_>,
    config: &MyConfig,
) -> (
    Vec<AsanaProject>,
    Vec<AsanaProjectSections>,
    Vec<AsanaProjectTaskGids>,
) {
    let mut project_futures = Vec::new();
    let mut project_sections_futures = Vec::new();
    let mut project_task_gids_futures = Vec::new();

    for (_, project_config) in &config.projects {
        project_futures.push(client.get_project(&project_config.gid));
        project_sections_futures.push(client.get_project_sections(&project_config.gid));
        project_task_gids_futures
            .push(client.get_project_task_gids(&project_config.gid, &project_config.horizon));
    }

    return join3(
        join_all(project_futures),
        join_all(project_sections_futures),
        join_all(project_task_gids_futures),
    )
    .await;
}

async fn get_asana_data_tasks(
    client: &AsanaClient<'_>,
    task_gids: &Vec<&String>,
) -> (Vec<AsanaTask>, Vec<AsanaTaskStories>) {
    let mut task_futures = Vec::new();
    let mut task_stories_futures = Vec::new();

    for task_gid in task_gids {
        task_futures.push(client.get_task(&task_gid));
        task_stories_futures.push(client.get_task_stories(&task_gid));
    }

    return join(join_all(task_futures), join_all(task_stories_futures)).await;
}

async fn get_asana_data_users(
    client: &AsanaClient<'_>,
    user_gids: &HashSet<&String>,
) -> Vec<AsanaUser> {
    let mut user_futures = Vec::new();

    for user_gid in user_gids {
        user_futures.push(client.get_user(&user_gid));
    }

    return join_all(user_futures).await;
}
