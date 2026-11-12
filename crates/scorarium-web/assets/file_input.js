{
    let nextUploadId = 1;
    let runningTransfers = 0;

    // The transfers running in each form, whose submit buttons are held until the last one ends.
    const transfers = new WeakMap();

    const hold = (form) => {
        const running = transfers.get(form) ?? 0;
        transfers.set(form, running + 1);
        if (running > 0) {
            return;
        }
        for (const control of form.elements) {
            if (control.type === "submit" && !control.disabled) {
                control.disabled = true;
                control.dataset.held = "";
            }
        }
    };

    const release = (form) => {
        const running = transfers.get(form) - 1;
        transfers.set(form, running);
        if (running > 0) {
            return;
        }
        // Only the buttons that hold() disabled are enabled, so that a button the server drew
        // disabled stays disabled.
        for (const control of form.elements) {
            if ("held" in control.dataset) {
                control.disabled = false;
                delete control.dataset.held;
            }
        }
    };

    window.addEventListener("beforeunload", (event) => {
        if (runningTransfers > 0) {
            event.preventDefault();
            event.returnValue = true;
        }
    });

    const requests = new WeakMap();

    document.addEventListener("list:remove", (event) => {
        for (const root of event.target.querySelectorAll("[data-file-input]")) {
            requests.get(root)?.abort();
            discard(root);
        }
    });

    const urlField = (root) => root.querySelector("[data-url-field]");
    const stagedField = (root) => root.querySelector("[data-file-staged]");

    const write = (root, selector, text) => {
        for (const element of root.querySelectorAll(selector)) {
            element.textContent = text;
        }
    };

    const setMessage = (root, message) => {
        write(root, "[data-file-message]", message);
        urlField(root).classList.toggle("is-invalid", message !== "");
    };

    const setProgress = (root, percent) => {
        const progress = root.querySelector("[data-file-progress]");
        if (progress) {
            progress.setAttribute("aria-valuenow", percent);
            progress.querySelector(".progress-bar").style.width = `${percent}%`;
        }
        write(root, "[data-file-percent]", `${percent}%`);
    };

    const discard = (root) => {
        const id = stagedField(root)?.value;
        if (id) {
            fetch(`${root.dataset.url}/${encodeURIComponent(id)}/discard`, {
                method: "POST",
                headers: { "HX-Current-URL": location.href },
            });
        }
    };

    const start = (root, id, name, source) => {
        setMessage(root, "");
        write(root, "[data-file-busy] [data-file-name]", name);
        setProgress(root, 0);
        const fetching = source === "url";
        root.querySelector("[data-file-progress]")?.toggleAttribute("hidden", fetching);
        root.querySelector("[data-file-percent]")?.toggleAttribute("hidden", fetching);
        root.querySelector("[data-file-spinner]")?.toggleAttribute("hidden", !fetching);
        root.dataset.state = "busy";
        root.dispatchEvent(new CustomEvent("upload:start", { bubbles: true, detail: { id, name, source } }));
    };

    const finish = (root, id, result) => {
        const staged = stagedField(root);
        if (result.ok) {
            const { id: stagedId, name } = JSON.parse(result.body);
            discard(root);
            if (staged) {
                staged.value = stagedId;
            }
            write(root, "[data-file-staged-name] [data-file-name]", name);
            root.dataset.state = "staged";
        } else {
            root.dataset.state = staged?.value ? "staged" : "empty";
            setMessage(root, result.message);
        }
        root.dispatchEvent(new CustomEvent("upload:end", { bubbles: true, detail: { id, ...result } }));
    };

    const post = (root, id, form) => {
        const request = new XMLHttpRequest();
        const owner = root.closest("form");
        const fail = (message) => {
            if (form.has("url")) {
                urlField(root).value = form.get("url");
            }
            finish(root, id, { ok: false, message });
        };
        // A fetch has no progress to show, because the browser cannot see the server's download.
        if (form.has("file")) {
            request.upload.addEventListener("progress", (event) => {
                if (event.lengthComputable) {
                    const { loaded, total } = event;
                    setProgress(root, Math.floor((loaded / total) * 100));
                    root.dispatchEvent(
                        new CustomEvent("upload:progress", { bubbles: true, detail: { id, loaded, total } }),
                    );
                }
            });
        }
        request.open("POST", root.dataset.url);
        // The server builds the login URL that returns to this page from the header, as for htmx.
        request.setRequestHeader("HX-Current-URL", location.href);
        request.addEventListener("load", () => {
            if (request.status >= 200 && request.status < 300) {
                finish(root, id, { ok: true, body: request.responseText });
                return;
            }
            const redirect = request.getResponseHeader("HX-Redirect");
            if (request.status === 401 && redirect) {
                location.href = redirect;
            }
            // A reverse proxy's own error page lacks the marker, which scorarium shouldn't display
            const page = new DOMParser().parseFromString(request.responseText, "text/html");
            const error = page.querySelector("[data-api-error]");
            fail(error ? error.textContent.trim() : `The upload failed (${request.status}).`);
        });
        request.addEventListener("error", () => fail("The upload failed."));
        // The loadend event fires exactly once after a load, error, abort, or timeout.
        request.addEventListener("loadend", () => {
            if (requests.get(root) === request) {
                requests.delete(root);
            }
            runningTransfers--;
            if (owner) {
                release(owner);
            }
        });
        runningTransfers++;
        if (owner) {
            hold(owner);
        }
        requests.set(root, request);
        request.send(form);
    };

    const upload = (root, file) => {
        const maxSize = Number(root.dataset.maxSize);
        if (file.size > maxSize) {
            setMessage(root, `The file is larger than ${maxSize / 1024 / 1024} MB.`);
            return;
        }
        const id = nextUploadId++;
        start(root, id, file.name, "file");
        const form = new FormData();
        form.append("file", file);
        post(root, id, form);
    };

    // Send each file as its own transfer when the input allows several, and refuse extra files
    // otherwise.
    const uploadAll = (root, files) => {
        if (files.length > 1 && !("multiple" in root.dataset)) {
            setMessage(root, "Paste or drop one file.");
            return;
        }
        Array.from(files).forEach((file) => upload(root, file));
    };

    const isDisabled = (root) => urlField(root).disabled;

    const fetchUrl = (root, field) => {
        const url = field.value.trim();
        field.value = "";
        const id = nextUploadId++;
        start(root, id, url, "url");
        const form = new FormData();
        form.append("url", url);
        post(root, id, form);
    };

    document.addEventListener("click", (event) => {
        const browse = event.target.closest("[data-file-input] [data-browse]");
        if (browse) {
            browse.closest("[data-file-input]").querySelector("input[type=file]").click();
        }
        const clear = event.target.closest("[data-drop-zone] [data-file-clear]");
        if (clear) {
            const root = clear.closest("[data-drop-zone]").querySelector("[data-file-input]");
            discard(root);
            const staged = stagedField(root);
            if (staged) {
                staged.value = "";
            }
            setMessage(root, "");
            root.dataset.state = "empty";
        }
    });

    document.addEventListener("change", (event) => {
        const root = event.target.closest("[data-file-input]");
        if (!root) {
            return;
        }
        if (event.target.matches("[data-url-field]")) {
            if (event.target.value.trim() !== "") {
                fetchUrl(root, event.target);
            }
            return;
        }
        if (event.target.type !== "file") {
            return;
        }
        // Copy the files first, because clearing the chooser empties its list. Clearing lets the
        // same file be chosen again after a failure.
        const files = Array.from(event.target.files);
        event.target.value = "";
        uploadAll(root, files);
    });

    document.addEventListener("keydown", (event) => {
        const root = event.target.closest("[data-file-input]");
        if (!root || !event.target.matches("[data-url-field]") || event.key !== "Enter") {
            return;
        }
        event.preventDefault();
        if (event.target.value.trim() !== "") {
            fetchUrl(root, event.target);
        }
    });

    document.addEventListener("paste", (event) => {
        const root = event.target.closest("[data-file-input]");
        if (!root || !event.target.matches("[data-url-field]") || isDisabled(root)) {
            return;
        }
        const field = event.target;
        const files = event.clipboardData.files;
        if (files.length > 0) {
            event.preventDefault();
            uploadAll(root, files);
            return;
        }
        // The pasted text has not landed in the field yet.
        setTimeout(() => {
            if (field.value.trim() !== "") {
                fetchUrl(root, field);
            }
        });
    });

    // Find the drop zone that contains the target, unless its file input is disabled.
    const dropZoneAt = (target) => {
        const zone = target.closest("[data-drop-zone]");
        const root = zone?.querySelector("[data-file-input]");
        return root && !isDisabled(root) ? zone : null;
    };

    const carriesDroppable = (event) => {
        const types = Array.from(event.dataTransfer?.types ?? []);
        return types.includes("Files") || types.includes("text/uri-list");
    };

    // The enter and leave events fire for every child, so the depth is counted to know when the
    // drag leaves the page.
    let dragDepth = 0;

    const textFieldSelector = [
        "textarea",
        "input:not([type])",
        "input[type=text], input[type=search], input[type=url], input[type=email]",
        "input[type=tel], input[type=password], input[type=number]",
        "[contenteditable]:not([contenteditable=false])",
    ].join(", ");

    // Text dropped into a text field is harmless, but a file has no ordinary drop there.
    const isTextDrop = (event) =>
        !event.dataTransfer.types.includes("Files") && event.target.closest(textFieldSelector);

    const setActiveZone = (zone) => {
        document.querySelectorAll("[data-drop-active]").forEach((other) => {
            if (other !== zone) {
                delete other.dataset.dropActive;
            }
        });
        if (zone) {
            zone.dataset.dropActive = "";
        }
    };

    const endDrag = () => {
        dragDepth = 0;
        setActiveZone(null);
        delete document.documentElement.dataset.dropTargets;
    };

    document.addEventListener("dragenter", (event) => {
        if (carriesDroppable(event)) {
            dragDepth++;
            document.documentElement.dataset.dropTargets = "";
        }
    });

    document.addEventListener("dragleave", (event) => {
        if (carriesDroppable(event) && --dragDepth <= 0) {
            endDrag();
        }
    });

    document.addEventListener("dragend", endDrag);

    document.addEventListener("dragover", (event) => {
        if (!carriesDroppable(event)) {
            return;
        }
        const zone = dropZoneAt(event.target);
        setActiveZone(zone);
        if (!zone && isTextDrop(event)) {
            return;
        }
        // An uncancelled dragover makes the browser navigate to a dropped file and discard unsaved
        // edits, so every other target refuses the drop explicitly instead.
        event.preventDefault();
        event.dataTransfer.dropEffect = zone ? "copy" : "none";
    });

    document.addEventListener("drop", (event) => {
        endDrag();
        if (!carriesDroppable(event)) {
            return;
        }
        const zone = dropZoneAt(event.target);
        if (!zone && isTextDrop(event)) {
            return;
        }
        event.preventDefault();
        if (!zone) {
            return;
        }
        const root = zone.querySelector("[data-file-input]");
        const files = event.dataTransfer.files;
        if (files.length > 0) {
            uploadAll(root, files);
        } else {
            const url = event.dataTransfer
                .getData("text/uri-list")
                .split(/\r?\n/)
                .find((entry) => /^https?:\/\//.test(entry));
            if (url) {
                const field = urlField(root);
                field.value = url;
                fetchUrl(root, field);
            }
        }
    });
}
