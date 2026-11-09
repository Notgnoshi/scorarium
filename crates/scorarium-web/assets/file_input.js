{
    let nextUploadId = 1;

    const finish = (root, id, result) => {
        root.dispatchEvent(new CustomEvent("upload:end", { bubbles: true, detail: { id, ...result } }));
    };

    const upload = (root, file) => {
        const id = nextUploadId++;
        root.dispatchEvent(new CustomEvent("upload:start", { bubbles: true, detail: { id, name: file.name } }));

        const maxSize = Number(root.dataset.maxSize);
        if (file.size > maxSize) {
            finish(root, id, { ok: false, message: `The file is larger than ${maxSize / 1024 / 1024} MB.` });
            return;
        }

        const request = new XMLHttpRequest();
        request.open("POST", root.dataset.url);
        // The server builds the login URL that returns to this page from the header, as for htmx.
        request.setRequestHeader("HX-Current-URL", location.href);
        request.addEventListener("load", () => {
            if (request.status >= 200 && request.status < 300) {
                finish(root, id, { ok: true, html: request.responseText });
                return;
            }
            const redirect = request.getResponseHeader("HX-Redirect");
            if (request.status === 401 && redirect) {
                location.href = redirect;
            }
            // A reverse proxy's own error page lacks the marker, which scorarium shouldn't display
            const page = new DOMParser().parseFromString(request.responseText, "text/html");
            const error = page.querySelector("[data-api-error]");
            const message = error ? error.textContent.trim() : `The upload failed (${request.status}).`;
            finish(root, id, { ok: false, message });
        });
        request.addEventListener("error", () => {
            finish(root, id, { ok: false, message: "The upload failed." });
        });
        const form = new FormData();
        form.append("file", file);
        request.send(form);
    };

    document.addEventListener("click", (event) => {
        const browse = event.target.closest("[data-file-input] [data-browse]");
        if (browse) {
            browse.closest("[data-file-input]").querySelector("input[type=file]").click();
        }
    });

    document.addEventListener("change", (event) => {
        const root = event.target.closest("[data-file-input]");
        if (!root || event.target.type !== "file") {
            return;
        }
        const [file] = event.target.files;
        // Clearing the chooser lets the same file be chosen again after a failure.
        event.target.value = "";
        if (file) {
            upload(root, file);
        }
    });
}
