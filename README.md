# Bakkutteh 🇸🇬 (WIP)

A CLI tool to create K8S Job from CronJob spec by allowing you to replace the environment variable (literal only) when needed.

## Usage

### Apply

Create a config from an existing cronjob

```sh
cargo run -- -j example-cronjob -t jojo
```

### Dry Run

You can use the dry run to show what will be applied into the K8S cluster

```sh
cargo run -- -j example-cronjob --dry-run -t jojo
```
