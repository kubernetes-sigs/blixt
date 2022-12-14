# UDP Test Server

This is a basic UDP server for testing UDP traffic in Blixt.

The program will listen on ports `9875`, `9876`, and `9877` for UDP datagrams
and will print diagnostic information about the datagrams.

For instance, if you were to send the text "test" to the server like this:

```console
$ echo "test" | nc -u 172.17.0.2 9875
```

The server would log this:

```console
$ docker run -it ghcr.io/kong/blixt-udp-test-server
waiting for listeners...
UDP worker listening on port 9876
UDP worker listening on port 9875
UDP worker listening on port 9877
health check server listening on 9878
port 9875: 5 bytes received from 172.17.0.1:34276
port 9875: buffer contents: test
```
