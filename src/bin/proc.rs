use metrics::config::*;

use chrono::{Date, DateTime, Datelike, TimeZone, Utc, Weekday};
use clap::{App, Arg};
use env_logger;
use lazy_static::lazy_static;
use metrics::asana::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::fs::File;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

fn main() {
    /* Logging */
    env_logger::init();

    /* Command Line */
    let (config_file_str, input_file_str, output_dir_str) = process_command_line();

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
     * Input file -- output of `fetch` program
     */
    let input_file_path = Path::new(&input_file_str)
        .canonicalize()
        .expect(&format!("Bad input file path: {}", &input_file_str));
    let input_str =
        fs::read_to_string(input_file_path).expect(&format!("Bad token file: {}", &input_file_str));
    let data: AsanaData = serde_json::from_str(&input_str).expect("Invalid output.");

    /*
     * Output
     */
    let mut output_dir_path = PathBuf::from(".");
    output_dir_path.push(output_dir_str);

    match fs::metadata(&output_dir_path) {
        Ok(dir_metadata) => {
            if !dir_metadata.is_dir() {
                panic!(
                    "Output dir path {} is not a dir",
                    &output_dir_path.to_str().unwrap()
                );
            }
        }
        Err(_) => {
            fs::create_dir_all(&output_dir_path).expect("Could not create output directory");
        }
    }
    let output_dir_path = output_dir_path.canonicalize().expect(
        format!(
            "Directoruy {} should exist",
            output_dir_path.to_str().unwrap()
        )
        .as_str(),
    );

    /*
     * Process
     */
    let report = proc_data(&config, &data);

    for report_project in report.projects {
        output_gnuplot_data(&report_project, &output_dir_path);
    }
}

fn process_command_line() -> (String, String, String) {
    let matches = App::new("proc")
        .version("0.1.0")
        .author("Parijat Mishra <parijat.mishra@gmail.com>")
        .about("Process Output of `fetch`")
        .arg(
            Arg::with_name("config-file")
                .short("c")
                .long("config-file")
                .takes_value(true)
                .help("path to config file"),
        )
        .arg(
            Arg::with_name("input-file")
                .short("i")
                .long("input-file")
                .takes_value(true)
                .help("path of file containing the output of the `fetch` program."),
        )
        .arg(
            Arg::with_name("output-dir")
                .short("o")
                .long("output-directory")
                .takes_value(true)
                .help("path to directory where output files will be stored"),
        )
        .get_matches();
    let config_file_str = matches
        .value_of("config-file")
        .expect("Flag --config-file=PATH must be specified");
    let input_file_str = matches
        .value_of("input-file")
        .expect("Flag --input-file=PATH must be specified");
    let output_dir_str = matches
        .value_of("output-dir")
        .expect("Flag --output-dir=DIRPATH must be specified");
    return (
        config_file_str.to_owned(),
        input_file_str.to_owned(),
        output_dir_str.to_owned(),
    );
}

#[derive(Debug)]
struct Report<'a> {
    projects: Vec<Project<'a>>,
}

#[derive(Debug)]
struct Project<'a> {
    label: &'a str,
    name: &'a str,
    cfd: Cfd<'a>,
}

#[derive(Debug)]
struct Cfd<'a> {
    cfd_states: Vec<&'a str>,
    done_states: Vec<&'a str>,
    period_counts: Vec<PeriodCounts>,
    period_durations: Vec<PeriodDurations>,
}

#[derive(Debug)]
struct PeriodCounts {
    date: Date<Utc>,
    cfd_state_counts: Vec<u32>,
    done_count: u32,
}

#[derive(Debug)]
struct PeriodDurations {
    date: Date<Utc>,
    p90_duration_seconds: Vec<u64>,
}

fn proc_data<'a>(config: &'a MyConfig, asana_data: &'a AsanaData) -> Report<'a> {
    let pnames: HashSet<&str> = get_data_pnames(asana_data);
    let pgid2pname: HashMap<&str, &str> = get_pgid2pname(asana_data);
    let sgid2sname: HashMap<&str, &str> = get_sgid2sname(asana_data);
    let tgid2asana_task: HashMap<&str, &AsanaTask> = get_tgid2asana_task(asana_data);
    let sgid2pgid: HashMap<&str, &str> = get_sgid2pgid(asana_data);
    let tgid2pname2sname: HashMap<&str, HashMap<&str, &str>> =
        get_tgid2pname2sname(&sgid2pgid, &sgid2sname, &pgid2pname, asana_data);

    // capture the times when a task entered a state ("section")
    // project_name => Vec<(event_time, task gid, state)>
    let mut pname2t_events: HashMap<&str, Vec<(&DateTime<Utc>, &str, &str)>> = get_task_events(
        &pnames,
        &tgid2asana_task,
        &tgid2pname2sname,
        &asana_data.task_stories,
    );

    let mut projects: Vec<Project> = Vec::new();

    for (label, project_config) in &config.projects {
        println!("Processing: {}", label);
        let pgid = project_config.gid.as_str();
        let pname: &str = pgid2pname[pgid];
        let cfd_states: Vec<&str> = project_config
            .cfd_states
            .iter()
            .map(|s| s.as_str())
            .collect();
        let done_states: Vec<&str> = project_config
            .done_states
            .iter()
            .map(|s| s.as_str())
            .collect();
        let horizon = &project_config.horizon.iso_week();
        let events: Vec<(&DateTime<Utc>, &str, &str)> = pname2t_events.remove(pname).unwrap();

        let mut cfd_period_counts: Vec<PeriodCounts> = Vec::new();
        let mut cfd_period_durations: Vec<PeriodDurations> = Vec::new();

        // ----
        // last know state of each task, and the timestamp when task entered that state
        let mut task_latest_state: HashMap<&str, (&str, &DateTime<Utc>)> = HashMap::new();
        // how many tasks are in each state at the moment
        let mut state_taskcounts: HashMap<&str, u32> = HashMap::new();
        // *in this period* how much time did tasks spend in this state
        let mut state_period_dwelltimes: HashMap<&str, Vec<u64>> = HashMap::new();
        // *in this period* how many tasks are in states considered to be "Done"
        // note - there can be multiple states that are considered to conceptually
        // be Done
        let mut done_count: u32 = 0;

        // ----
        let mut start_of_period = Utc
            .isoywd(horizon.year(), horizon.week(), Weekday::Mon)
            .and_hms(0, 0, 0);
        let mut start_of_next_period = start_of_period
            .checked_add_signed(chrono::Duration::weeks(1))
            .unwrap();
        // ----

        for (at, task_gid, sname) in events.into_iter() {
            while at >= &start_of_next_period {
                // event in next period -- finalize this period stats and rollover to next period
                // task -> state ==> count how many times each state appeared
                for (sname, &timestamp) in task_latest_state.values() {
                    let count = state_taskcounts.entry(sname).or_insert_with(|| 0);
                    *count += 1;

                    let dwelltime = (start_of_next_period - timestamp).num_seconds() as u64;
                    state_period_dwelltimes
                        .entry(sname)
                        .or_insert_with(|| Vec::new())
                        .push(dwelltime);
                }
                // extract the counts of the subset of states in `p_counted_states`
                let state_count_vec: Vec<u32> = cfd_states
                    .iter()
                    .map(|&k| *state_taskcounts.get(k).unwrap_or(&0))
                    .collect();
                let period_counts = PeriodCounts {
                    date: start_of_period.date(),
                    cfd_state_counts: state_count_vec,
                    done_count: done_count,
                };
                cfd_period_counts.push(period_counts);

                // extract the P90 duration of the subsets of states in `p_counted_states`
                let p90_duration_seconds: Vec<u64> = cfd_states
                    .iter()
                    .map(|&k| {
                        state_period_dwelltimes
                            .get_mut(k)
                            .map(|vec| {
                                vec.sort_unstable();
                                p90(vec)
                            })
                            .unwrap_or(0)
                    })
                    .collect();
                let period_durations = PeriodDurations {
                    date: start_of_period.date(),
                    p90_duration_seconds: p90_duration_seconds,
                };
                cfd_period_durations.push(period_durations);

                // clear the state_durations because we only count the time
                // tasks spend in a state within a period
                state_period_dwelltimes.clear();

                // reset done_count because we only count tasks done
                // within this period
                done_count = 0;

                // update loop variables for next period
                start_of_period = start_of_period
                    .checked_add_signed(chrono::Duration::weeks(1))
                    .unwrap();
                start_of_next_period = start_of_next_period
                    .checked_add_signed(chrono::Duration::weeks(1))
                    .unwrap();
            }
            // event in current period
            if let Some((old_state, old_at)) = task_latest_state.insert(task_gid, (sname, at)) {
                let old_state_duration_seconds = (*at - *old_at).num_seconds() as u64;
                state_period_dwelltimes
                    .entry(old_state)
                    .or_insert_with(|| Vec::new())
                    .push(old_state_duration_seconds);
            }
            if done_states.contains(&sname) {
                done_count += 1;
            }
        }

        let project = Project {
            label: label,
            name: pname,
            cfd: Cfd {
                cfd_states: cfd_states,
                done_states: done_states,
                period_counts: cfd_period_counts,
                period_durations: cfd_period_durations,
            },
        };
        projects.push(project);
    }
    let report = Report { projects };

    return report;
}

fn p90(vec: &Vec<u64>) -> u64 {
    let idx = ((vec.len() - 1) as f64 * 0.9) as usize;
    return *vec.iter().nth(idx).unwrap();
}

fn get_data_pnames(asana_data: &AsanaData) -> HashSet<&str> {
    return asana_data
        .projects
        .iter()
        .map(|AsanaProject { name, .. }| name.as_str())
        .collect();
}

fn get_pgid2pname(asana_data: &AsanaData) -> HashMap<&str, &str> {
    return asana_data
        .projects
        .iter()
        .map(|AsanaProject { gid, name, .. }| (gid.as_str(), name.as_str()))
        .collect();
}

fn get_sgid2sname(asana_data: &AsanaData) -> HashMap<&str, &str> {
    return asana_data
        .project_sections
        .iter()
        .flat_map(|aps| {
            aps.sections
                .iter()
                .map(|a_s| (a_s.gid.as_str(), a_s.name.as_str()))
        })
        .collect();
}

fn get_tgid2asana_task(asana_data: &AsanaData) -> HashMap<&str, &AsanaTask> {
    return asana_data
        .tasks
        .iter()
        .map(|t| (t.gid.as_str(), t))
        .collect();
}

fn get_sgid2pgid(asana_data: &AsanaData) -> HashMap<&str, &str> {
    return asana_data
        .project_sections
        .iter()
        .flat_map(|aps| {
            aps.sections
                .iter()
                .map(move |a_s| (a_s.gid.as_str(), aps.project_gid.as_str()))
        })
        .collect();
}

fn get_tgid2pname2sname<'a>(
    sgid2pgid: &HashMap<&'a str, &'a str>,
    sgid2sname: &HashMap<&'a str, &'a str>,
    pgid2pname: &HashMap<&'a str, &'a str>,
    asana_data: &'a AsanaData,
) -> HashMap<&'a str, HashMap<&'a str, &'a str>> {
    let tgid2sgids: HashMap<&str, Vec<&str>> = asana_data
        .tasks
        .iter()
        .map(|a_t| {
            (
                a_t.gid.as_str(),
                a_t.memberships
                    .iter()
                    .map(|hm| hm["section"].gid.as_str())
                    // AsanaTask.membership lists sections from *all* projects a task is in
                    // not just the ones we are interested in, so filter out the sections
                    // that con't exist in our `project_sections`
                    .filter(|sgid| sgid2pgid.contains_key(*sgid))
                    .collect(),
            )
        })
        .collect();

    let tgid2pname2sname = tgid2sgids
        .iter()
        .map(|(tgid, vec_sgid)| {
            (
                *tgid,
                vec_sgid
                    .iter()
                    .map(|sgid| (pgid2pname[sgid2pgid[sgid]], sgid2sname[sgid]))
                    .collect(),
            )
        })
        .collect();

    return tgid2pname2sname;
}

fn get_task_events<'a>(
    pnames: &'a HashSet<&str>,
    tgid2asana_task: &'a HashMap<&str, &AsanaTask>,
    tgid2pname2sname: &'a HashMap<&str, HashMap<&str, &str>>,
    task_stories: &'a Vec<AsanaTaskStories>,
) -> HashMap<&'a str, Vec<(&'a DateTime<Utc>, &'a str, &'a str)>> {
    let mut pname2t_events: HashMap<&str, Vec<(&DateTime<Utc>, &str, &str)>> = HashMap::new();

    // read all the stories and convert them into a timeline of events per project
    for asana_task_story in task_stories {
        let task_gid: &str = asana_task_story.task_gid.as_str();
        let task_created_at = &tgid2asana_task[task_gid].created_at;

        for asana_story in &asana_task_story.stories {
            if asana_story.resource_subtype.eq("section_changed") {
                // parse the text of the story
                let (sname_from, sname_to, pname) = parse_section_changed(&asana_story.text);
                // event may be for a project we are not interested in
                if pnames.contains(pname) {
                    let section_changed_at: &DateTime<Utc> = &asana_story.created_at;
                    let events = pname2t_events.entry(pname).or_insert_with(|| Vec::new());

                    // if a previous event for this task does not exist, it means we are
                    // looking at the first section change event -- in that case
                    // we assume that the task existed in the `sname_from` section at creation.
                    if events.is_empty() {
                        events.push((&task_created_at, task_gid, sname_from));
                    }
                    // insert the event for section the task moved to
                    events.push((section_changed_at, task_gid, sname_to));
                }
            }
        }

        // if a task never changed sections after creation, there is no "section changed" story
        // so we look for such tasks and synthesize the "create" story
        for pname in tgid2pname2sname[task_gid].keys() {
            let events = pname2t_events.entry(pname).or_insert_with(|| Vec::new());
            if events.is_empty() {
                let task_curr_sname = tgid2pname2sname[task_gid][pname];
                events.push((task_created_at, task_gid, task_curr_sname));
            }
            events.sort_by_cached_key(|entry| entry.0);
        }
    }

    return pname2t_events;
}

fn parse_section_changed(text: &str) -> (&str, &str, &str) {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r#"^moved this Task from "([^"]+?)" to "([^"]+?)" in (.+)$"#).unwrap();
    }
    let caps = RE.captures(text).unwrap();
    return (
        caps.get(1).unwrap().as_str(),
        caps.get(2).unwrap().as_str(),
        caps.get(3).unwrap().as_str(),
    );
}

fn output_gnuplot_data(report_project: &Project, output_dir_path: &Path) {
    let name = report_project.name;
    let label = report_project.label;

    println!("Output for {}: {}", label, name);

    let cfd_states = &report_project.cfd.cfd_states;
    let done_states = &report_project.cfd.done_states;

    // ---------
    // CFD Data File
    // ---------
    let mut buffer = String::new();
    // header
    write!(&mut buffer, "# date").unwrap();
    for state in cfd_states {
        write!(&mut buffer, " \"{}\"", state).unwrap();
    }
    write!(&mut buffer, "\n").unwrap();
    // record
    for period_count in report_project.cfd.period_counts.iter() {
        let date = period_count.date;
        write!(
            &mut buffer,
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        )
        .unwrap();

        for count in period_count.cfd_state_counts.iter() {
            write!(&mut buffer, " {}", count).unwrap();
        }
        write!(&mut buffer, "\n").unwrap();
    }
    // data file
    let cfd_data_file_name = format!("{}_cfd.dat", label);
    let mut cfd_data_file_path = PathBuf::from(output_dir_path);
    cfd_data_file_path.push(&cfd_data_file_name);
    File::create(&cfd_data_file_path)
        .unwrap()
        .write_all(buffer.as_bytes())
        .unwrap();
    println!("Wrote {}", cfd_data_file_path.to_str().unwrap());

    // ---------
    // P90 Durations Data File
    // ---------
    let mut buffer = String::new();
    // header
    write!(&mut buffer, "# date").unwrap();
    for state in cfd_states {
        write!(&mut buffer, " \"{}\"", state).unwrap();
    }
    write!(&mut buffer, "\n").unwrap();
    // record
    for period_durations in report_project.cfd.period_durations.iter() {
        let date = period_durations.date;
        write!(
            &mut buffer,
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        )
        .unwrap();
        for duration in period_durations.p90_duration_seconds.iter() {
            write!(
                &mut buffer,
                " {}",
                (*duration as f32) / (24.0 * 60.0 * 60.0)
            )
            .unwrap();
        }
        write!(&mut buffer, "\n").unwrap();
    }
    // data file
    let duration_data_file_name = format!("{}_p90_durations.dat", label);
    let mut duration_data_file_path = PathBuf::from(output_dir_path);
    duration_data_file_path.push(&duration_data_file_name);
    File::create(&duration_data_file_path)
        .unwrap()
        .write_all(buffer.as_bytes())
        .unwrap();
    println!("Wrote {}", duration_data_file_path.to_str().unwrap());

    // ---------
    // Done Count Data File
    // ---------
    let mut buffer = String::new();
    // header
    writeln!(&mut buffer, "# date done_count").unwrap();
    // record
    for period_counts in report_project.cfd.period_counts.iter() {
        let date = period_counts.date;
        let done_count = period_counts.done_count;
        writeln!(
            &mut buffer,
            "{:04}-{:02}-{:02} {}",
            date.year(),
            date.month(),
            date.day(),
            done_count
        )
        .unwrap();
    }
    // data file
    let done_count_data_file_name = format!("{}_done.dat", label);
    let mut done_count_data_file_path = PathBuf::from(output_dir_path);
    done_count_data_file_path.push(&done_count_data_file_name);
    File::create(&done_count_data_file_path)
        .unwrap()
        .write_all(buffer.as_bytes())
        .unwrap();
    println!("Wrote {}", done_count_data_file_path.to_str().unwrap());

    // ---------
    // Gnuplot
    // ---------
    let mut buffer = String::new();
    writeln!(
        &mut buffer,
        r#"
set terminal png enhanced font "Arial,10" fontscale 1.0 size 1024,768
set output "{label}.png"
set multiplot layout 3,1 title "{name}""#,
        label = label,
        name = name
    )
    .unwrap();
    // CFD - Counts
    writeln!(
        &mut buffer,
        r#"# CFD
set title "Cumulative Tasks in State - Count"
set key left top outside
set xdata time
set timefmt "%Y-%m-%d"
{plotline}"#,
        plotline = make_gnuplot_cfdline(&cfd_data_file_name, &cfd_states)
    )
    .unwrap();
    // P90 Durations (Hours)
    writeln!(
        &mut buffer,
        r#"# P90 Duration (Days)
set title "P90 Age Tasks in State - Days"
set key left top outside
set xdata time
set timefmt "%Y-%m-%d"
{plotline}"#,
        plotline = make_gnuplot_cfdline(&duration_data_file_name, &cfd_states)
    )
    .unwrap();
    // Task "Done" per period
    writeln!(
        &mut buffer,
        r#"# Tasks "Done" per period
set title "Throughput - Tasks Transitioning Into {done_state_names} - Count"
unset key
set xdata time
set timefmt "%Y-%m-%d"
plot "{data_file_name}" using 1:2 with filledcurve x1"#,
        done_state_names = done_states.join(", "),
        data_file_name = done_count_data_file_name
    )
    .unwrap();

    // gnuplot file
    let gnuplot_file_name = format!("{}.gnuplot", label);
    let mut gnuplot_file_path = PathBuf::from(output_dir_path);
    gnuplot_file_path.push(&gnuplot_file_name);
    let mut gf = File::create(&gnuplot_file_path).unwrap();
    gf.write_all(buffer.as_bytes()).unwrap();
    println!("Wrote {}", gnuplot_file_path.to_str().unwrap());
}

fn make_gnuplot_cfdline(file_name: &str, states: &Vec<&str>) -> String {
    let mut buffer = String::from("plot");
    // gnuplot: columns in data files start from 1
    // col 1 is the date col; state cols are 2, 3, ... states.len() + 1
    let max_gnuplot_col = states.len() + 1;
    for (idx, state) in states.iter().enumerate() {
        // idx starts from 0
        if idx > 0 {
            write!(&mut buffer, ",").unwrap()
        };
        let gnuplot_column = idx + 2;
        write!(
            &mut buffer,
            r#" "{file_name}" using 1:({col}) with filledcurve x1 title "{state}""#,
            file_name = file_name,
            col = make_col_expression(gnuplot_column as u32, max_gnuplot_col as u32),
            state = state
        )
        .unwrap();
    }
    write!(&mut buffer, "\n").unwrap();
    return buffer;
}

fn make_col_expression(cur_col: u32, max_col: u32) -> String {
    // return "$<cur_col>+$<cur_col+1>+...$max_col"
    let mut buffer = String::new();
    for i in cur_col..=max_col {
        if i > cur_col {
            write!(&mut buffer, "+").unwrap();
        };
        write!(&mut buffer, "${}", i).unwrap();
    }
    return buffer;
}
