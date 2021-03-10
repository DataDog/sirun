# Contributing

Contributions are welcome! Note the [licensing policy in the
README](https://github.com/DataDog/glommio#contribution), and if you add or
change any dependencies, be sure to update the `LICENSES-3rdparty.csv` file. You
can use the following command to help with this:

```sh
cargo license -j | node -e "JSON.parse(fs.readFileSync(0)+'').map(l=>console.log([l.name,l.repository,l.license,l.authors]+''))" | grep -v sirun
```
