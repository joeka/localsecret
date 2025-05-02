# localsecret
Share files like secrets via a simple http server from the command line.

Running the command `localsecret -s path/to/my/file.ext` will start a server binding to a local IP address and a dynamically assigned port.
An URL is printed to share the file in the local network.
The URL looks like this: `http://<IP>:<PORT>/<PREFIX>/<filename>`. Where the PREFIX is a randomly generated string. In our example it could look something like this: `http://10.11.12.13:46497/gegPK3qQTFhdSPooJ1bmeU0KGzKSzBkruqI30GXWyi/file.ext`.

Alternatively you can pipe the content into the program, e.g. like this:
``` bash 
localsecret < secret.txt
# or
echo "very secret" | localsecret
```

## Usage
```
Share secrets via a local http server

Usage: localsecret [OPTIONS]

Options:
  -s, --secret-file <SECRET_FILE>
          The secret file to share. If not set, expects the input to be piped to stdin
      --url-prefix-length <URL_PREFIX_LENGTH>
          Length of the randomly generated url prefix [default: 42]
      --uses <USES>
          How often the shared url can be used [default: 1]
      --failed-attempts <FAILED_ATTEMPTS>
          How some invalid url can be used before the server stops. Don't set this to 0, as browser e.g. try to fetch the favicon.ico file [default: 3]
      --bind-ip <BIND_IP>
          IP address to bind the server to. If not set, will try to find the local IP address
  -h, --help
          Print help
  -V, --version
          Print version
```