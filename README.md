# sirun 🚨

`sirun` (pronounced like "siren") is a tool for taking basic perfomance
measurements of a process covering its entire lifetime. It gets memory and
timing information from the kernel and also allows
[Statsd](https://github.com/statsd/statsd#usage) messages to be sent to
`udp://localhost:$SIRUN_STATSD_PORT` (the port is assigned randomly by sirun,
but you can also set it yourself), and those will be included in the outputted
metrics.

It's intended that this tool be used for shorter-running benchmarks, and not for
long-lived processes that don't die without external interaction. You could
certainly use it for long-lived processes, but that's not where it shines.

## Installation

### Via Cargo

`cargo install sirun`

### From releases

[Release bundles](https://github.com/DataDog/sirun/releases) are provided for
each supported plaform. Extract the binary somewhere and use it.

### From source

Make sure you have [`rustup`](https://rustup.rs/) installed, and use that to
ensure you have the latest stable Rust toolchain enabled.

#### From a local repo clone

`cargo install .`

#### Without cloning locally

With SSH

`cargo install --git ssh://git@github.com:22/DataDog/sirun.git --branch main`

or with HTTPS

`cargo install --git https://github.com/DataDog/sirun.git --branch main`

## Usage

_See also the [documentation](https://docs.rs/sirun/latest/)._

Create a JSON or YAML file with the following properties:

* **`name`**: This will be included in the results JSON.
* **`run`**: The command to run and test. You can format this like a shell
  command with arguments, but note that it will not use a shell as an
  intermediary process. Note that subprocesses will not be measured via the
  kernel, but they can still use Statsd. To send metrics to Statsd from inside
  this process, send them to `udp://localhost:$SIRUN_STATSD_PORT`.
* **`service`**: A command to start a process to be run alongside your test
  process. This is for, for example, running a web service for your program to
  call out to, or a load-generating tool for your program. It should generally
  be used in conjunction with `setup`, which can be used to determine whether
  the `service` process is ready. There is no retry logic. After the test run
  has completed, the process will be sent a SIGKILL.
* **`setup`**: A command to run _before_ the test. Use this to ensure the
  availability of services, or retrieve some last-minute dependencies. This can
  be formatted the same way as `run`. It will be run repeatedly at 1 second
  intervals until it exits with status code 0.
* **`teardown`**: A command to run _after_ the test. This is run in the same
  manner as `setup`, except after the test has run instead of before.
* **`timeout`**: If provided, this is the maximum time, in seconds, a `run` test
  can run for. If it times out, `sirun` will exit with no results, aborting the
  test.
* **`env`**: A set of environment variables to make available to the `run` and
  `setup` programs. This should be an object whose keys are the environment
  variable names and whose values are the environment variable values.
* **`iterations`**: The number of times to run the the `run` test. The results
  for each iteration will be in an `iterations` array in the resultant JSON. The
  default is 1.
* **`instructions`**: If set to `true`, will take instruction counts from
  hardware counters if available, adding the result under the key
  `instructions`, for each iteration. This is only available on Linux with
  `CAP_SYS_PTRACE`.
* **`variants`**: An array or object whose values are config objects, whose
  properties may be any of the properties above. It's not recommended to include
  `name` in a variant. The variant name (if `variants` is an object) or index
  (if `variants` is an array) will be included in resultant JSON.

### Environment Variables

* **`GIT_COMMIT_HASH`**: If set, will include a `version` in the
  results.
* **`SIRUN_NAME`**: If set, will include a `name` in the results. This overrides
  any `name` property set in config JSON/YAML.
* **`SIRUN_NO_STDIO`**: If set, supresses output from the tested program.
* **`SIRUN_VARIANT`**: Selects which variant of the test to run. If the
  `variants` property exists in the config JSON/YAML, and this variable is not
  set, then _all_ variants will be run, one-by-one, each having its own line of
  output JSON.
* **`SIRUN_STATSD_PORT`**: The UDP port on localhost to use for Statsd
  communication between tested processes and sirun. By default a random port
  will be assigned. You should read this variable from tested programs to
  determine which port to send data to.

### Example

Here's an example JSON file. As an example of a `setup` script, it's checking for
connectivity to Google. The `run` script doesn't do much, but it does send a
single metric (with name `udp.data` and value 50) via Statsd. It times out after
4 seconds, and we're not likely to reach that point.

There are two variants of this test, one named `control` and the other `with-tracer`.
The variants set environment variables, though the `run` script doesn't really care
about the variable. Since there are 3 iterations and 2 variants the `run` command will
run a total of 6 times.

```js
{
  "name": "foobar",
  "setup": "curl -I http://www.google.com -o /dev/null",
  "run": "bash -c \"echo udp.data:50\\|g > /dev/udp/127.0.0.1/$SIRUN_STATSD_PORT\"",
  "timeout": 4,
  "iterations": 3,
  "variants": {
    "control": {
      "env": { "USE_TRACER": "0" }
    },
    "with-tracer": {
      "env": { "USE_TRACER": "1" }
    }
  }
}
```

You can then pass this JSON file to `sirun` on the command line. Remember that
you can use environment variables to set the git commit hash and test name in
the output.

```sh
SIRUN_NAME=test_some_stuff GIT_COMMIT_HASH=123abc sirun ./my_benchmark.json
```

This will output something like the following.

```
  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
                                 Dload  Upload   Total   Spent    Left  Speed
  0     0    0     0    0     0      0      0 --:--:-- --:--:-- --:--:--     0
{"version":"123abc","name":"test_some_stuff",iterations:[{"user.time":6389.0,"system.time":8737.0,"udp.data":50.0,"max.res.size":2240512.0}]}
```

### Summaries

If you provide the `--summarize` option, `sirun` will switch to summary mode. In
summary mode, it will read from `stdin`, expecting line-by-line of output from
previous sirun runs. It will then aggregate them by test name and variant, and
provide summary statistics over iterations. The output is pretty-printed JSON.

E.g.

```bash
$ sirun foo-test.json >> results.ndjson
$ sirun bar-test.json >> results.ndjson
$ sirun baz-test.json >> results.ndjson
$ cat results.ndjson | sirun --summarize > summary.json
```

Each line of output in one of these `.ndjson` files is a complete JSON document.
Here's an example of one of these lines of output, though whitespace has been added for readability:

```json
{
    "name": "foobar",
    "variant": "with-baz",
    "iterations": [
        {
            "cpu.pct.wall.time": 18.138587328535383,
            "max.res.size": 66956,
            "system.time": 766091,
            "user.time": 1552557,
            "wall.time": 12782958
        },
        {
            "cpu.pct.wall.time": 18.029851850045276,
            "max.res.size": 66480,
            "system.time": 720491,
            "user.time": 1571242,
            "wall.time": 12710770
        }
    ]
}
```

- **`name`**: This is the same `name` value from the configuration file.
- **`variant`**: This is the object key from the configuration file's `variations` list.
- **`iterations`**: These are the statsd metrics from different runs and contain raw data
  - **`max.res.size`**: Kilobytes (KiB) maximum Resident Set Size (RSS), aka the highest RAM usage
  - **`system.time`**: Microsecond (μs) amount of time spent in kernel code
  - **`user.time`**: Microsecond (μs) amount of time spent in application code
  - **`wall.time`**: Microsecond (μs) amount of time the overall iteration took
  - **`cpu.pct.wall.time`**: Percentage (%) of time where the program was not waiting (`(user + system) / wall`)

The listed statsd metrics in this list are automatically created for you by Sirun.
Your application is free to emit other metrics as well.
Those additional metrics will also be provided in the output.

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
