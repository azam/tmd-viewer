# tmd-viewer

This is a viewer application for media data archived by Twitter Media Downloader. This is an API server and a single page web application as interface.

Server side (file scanning, media thumbnail generation, REST API) is implemented using [actix-web](https://actix.rs/) and vanilla Javascript for web interface. Provides thumbnail generation on-the-fly, and also pre-generated.

Web interface is a single web application with hash based page routing. Feel free to override the interface since the assets are all static. Bundled interface is built with [Bulma](https://bulma.io/), with custom CSS for dark mode.

## License

MIT License. See [LICENSE](LICENSE) file.

```
Copyright (c) 2022 Azamshul Azizy

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE
OR OTHER DEALINGS IN THE SOFTWARE.
```

## Compilation

1. Run `cargo build --release` and the executable `tmd-viewer` will be created on `target/release` directory.

## Usage

1. Edit `tmd-viewer.yaml`.

    Example:

    ```
    data_dir: "C:/data/twitter"
    bind_address: 127.0.0.1:8888
    scanner_count_limit: 2
    time_offset: 9
    ```

    * `data_dir`: A relative or absolute path to a directory where the archived twitter data is.
    * `bind_address`: Network interface and port to bind to. e.g. `127.0.0.1:8080` , `localhost:80`
    * `scanner_count_limit`: Scanner count limit. Keep this low at `1` or `2`, since a higher number have higher risk of database concurrency errors.
    * `time_offset`: The time offset in hours to use to read the archive files. e.g. if the archive files is created at Japan Standard Time (GMT+9), then set this at `9`.

2. Run the server `tmd-viewer` from this directory (or any directory that contains a `tmd-viewer.yaml` file and `static` directory).
3. Open the page on a browser.
4. Goto _Settings_.
5. Start by scanning the directory by clicking _Scan_.
6. When scan is completed you can go to the _Feeds_ tab to view the data available.

## Windows service

### Add as windows service

1. Make sure `tmd-viewer-service.exe`, `/static` and `tmd-viewer.yaml` is in the same directory.
2. Run the following command on as Administrator:
```
sc.exe create tmd-viewer-service binPath= "{PATH_TO_TMD_VIEWER}\tmd-viewer.exe" "service"
```

### Remove windows service

1. Run the following command on as Administrator:
```
sc.exe delete tmd-viewer-service
```

## Reference

* [Twitter Media Downloader](https://chrome.google.com/webstore/detail/twitter-media-downloader/cblpjenafgeohmnjknfhpdbdljfkndig?hl=en)