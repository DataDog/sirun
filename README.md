# sirun 🚨

`sirun` (pronounced like "siren") is a tool for taking basic perfomance
measurements of a process covering its entire lifetime. It gets memory and
timing information from the kernel and also allows
[Statsd](https://github.com/statsd/statsd#usage) messages to be sent to
`udp://localhost:8125` (in the future this will be configurable), and those will
be included in the outputted metrics.

It's intended that this tool be used for shorter-running benchmarks, and not for
long-lived processes that don't die without external interaction. You could
certainly use it for long-lived processes, but that's not where it shines.

## Installation


### From releases

[Release bundles](https://github.com/DataDog/sirun/releases) are provided for
each supported plaform. Extract the binary somewhere and use it.

### From source

Make sure you have [`rustup`](https://rustup.rs/) installed, and use that to
ensure you have the latest stable Rust toolchain enabled.

#### From a local repo clone

`cargo install .`

#### Without cloning locally

`cargo install --git git@github.com:DataDog/sirun.git`

## Usage

Create a JSON or YAML file with the following properties.

* **`run`**: The command to run and test. You can format this like a shell
  command with arguments, but note that it will not use a shell as an
  intermediary process. Note that subprocesses will not be measured via the
  kernel, but they can still use Statsd. To send metrics to Statsd from inside
  this process, send them to `udp://localhost:8125`.
* **`setup`**: A command to run _before_ the test. Use this to ensure the
  availability of services, or retrieve some last-minute dependencies. This can
  be formatted the same way as `run`. It will be run repeatedly at 1 second
  intervals until it exits with status code 0.
* **`timeout`**: If provided, this is the maximum time, in seconds, a `run` test
  can run for. If it times out, `sirun` will exit with no results, aborting the
  test.

### Environment Variables

* **`GIT_COMMIT_HASH`**: If set, will include a `version` in the
  results.
* **`SIRUN_NAME`**: If set, will include a `name` in the results.

### Example

Here's an example JSON file. As an example of a `setup` script, it's checking for
connectivity to Google. The `run` script doesn't do much, but it does send a
single metric (with name `udp.data` and value 50) via Statsd. It times out after
4 seconds, and we're not likely to reach that point.

```js
{
  "setup": "curl -I http://www.google.com -o /dev/null",
  "run": "bash -c \"echo udp.data:50\\|g > /dev/udp/127.0.0.1/8125\"",
  "timeout": 4
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
results: {"version": "123abc", "name": "test_some_stuff", "user.time": "6389", "system.time": "8737", "udp.data": "50", "max.res.size": "2240512"}
```

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
