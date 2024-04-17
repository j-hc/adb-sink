# adb-sink

```
github.com/j-hc
Version: 1.0
Usage: adb-sink <COMMAND>
Commands:
  pull  
  push  
  help  Print this message or the help of the given subcommand(s)
```

```
Usage: adb-sink pull [OPTIONS] <SOURCE> <DEST>

Arguments:
  <SOURCE>  
  <DEST>    

Options:
  -t, --set-times                set modified time of files
  -d, --delete-if-dne            delete files on target that does not exist in source
  -i, --ignore-dir <IGNORE_DIR>  ignore dirs starting with specified string
  -h, --help                     Print help

Options:
  -h, --help     Print help
  -V, --version  Print version

```