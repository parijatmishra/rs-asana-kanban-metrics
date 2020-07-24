# Asana Kanban Metrics Fetches

Given a project organized as a Kanban board, i.e., sections represents stages in a workflow, fetch task information and create some graphs.

Current graphs:

1. Cumulative Flow Diagram of Task Counts by "Stage"

    This graph shows, on a week by week basis, how many tasks are in each stage.  It is possible to see the "Lead Time" for tasks at a high level from this graph.  It also illustrates whether some stage is a bottleneck.
    
2. Cumulative Flow Diagram of P90 Task Age by "Stage"

    This graph shows, on a week by week basis, how *long* tasks stay in a particular stage. The value shown in the P90 age of all tasks in that stage. This is an alternative way to view the progress of tasks -- looking not at the number of tasks but how long they take.
    
3. Throughput.

    This graph shows, on a week by week basis, how many tasks are moved to "Done".

## Building

The code is written in Rust and built using Cargo. Install rust and cargo, and then:


    $ cargo build [--release]
    
This will create two binaries: `target/debug/fetch` is used to query the Asana API and download information.
    `target/debug/proc` is then run on the downloaded information to generate graphs.
    
    
## Configuration

Needs two items:

- An Asana Personal Access Token.  Get it from your Asana profile and put it in a file somewhere on the file system.
- A config file. A sample is given in `config.example.json`. Note: you can specify multiple projects.
    - "projects": an object, each of who keys is a friendly label / short name of a project, and whose value is an project config object. (Note: they label is not used anywhere in the output, only in debugging logs, so it does not have match the name in Asana - it can be any short string to aid in debugging.)
    - project config object:
        - "gid": (string) the Asana GID of the project. Can be obtained from inspecting the Asana URL of a project.
        - "horizon": (string containing a ISO8859 encoded timestamp) time from which the graphs should start; since projects can be very long lived and we are usually interested in recent last few months, horizon specifies how far back in time you want to go.
        - "cfd_stated": (array of strings) states to include in the Cumulative Flow Diagram. "States" are Asana section names  and must match exactly. The order of the states is the order in which the graph will show the states and are assumed to be from earlier stages first to later stages last.  Not all states in an Asana board may be relevant so include only those states which you want to show in the graphs.
        - "done_states": (array of strings) for throughput calculations, tasks in these states are considered to be "Done". Some boards may have multiple states equivalent to done so the value of this key is an array and not a single state name.
     
## Running it

To enable logging, set the environment variable RUST_LOG.

    $ export RUST_LOG=metrics=debug # or just RUST_LOG=debug -- very verbose

Fetch data from Asana:

    $ ./target/debug/fetch --help
    # assuming you have stored the Asana API personal access token at ~/.asana-personal-access-token
    $ ./target/debug/fetch --config-file my_config.json --token-file ~/.asana-personal-access-token --output-file asana_data.json

Process the fetched data to generate graphs (you need the `gnuplot` program installed)

    $ mkdir output
    $ ./target/debug/proc --config-file my_config.json --output output/

The output/ dir will contain a PNG file with some graphs, one for each project mentioned in the config file. There will also be some intermediate files needed for GnuPlot to do it's work.

## BUGS

- The `fetch` program does not seem to respect the `-o` parameter.
- The `proc` program does not seem to respect the `horizon` parameter.
