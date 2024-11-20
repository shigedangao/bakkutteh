# Bakkutteh 🇸🇬

A CLI tool to create K8S Job from CronJob spec by allowing you to replace the environment variable (literal only) when needed.

## Usage

> [!NOTE]
> Your kubeconfig need to be configured.

### Apply

Create a config from an existing cronjob

```sh
bakkutteh -j example-cronjob -t momo
```

### Dry Run

You can use the dry run to show what will be applied into the K8S cluster

```sh
bakkutteh -j example-cronjob --dry-run -t jojo
```

### Don't know the name of the CronJob you run ?

Just run without the option `-j`. A prompt will appear showing the list of cronjob allowing you to choose the list of cronjob that you're looking for

```sh
bakkutteh -- -t dodo --dry-run
```

### Output example

```sh
bakkutteh -t hello-dodo -n default
> Select the cronjob that you want to use as a base of the job my-cronjob
Getting cronjob my-cronjob from namespace default
> Env for ADDITIONAL_VAR:  hello dodo !
Job hello-dodo-manual created
```
