{
    const spinner = `
        <div class="position-absolute top-0 start-0 w-100 h-100 d-flex align-items-center justify-content-center bg-body bg-opacity-75" data-spinner>
            <div class="spinner-border" role="status"><span class="visually-hidden">Working</span></div>
        </div>`;

    const setBusy = (box, busy) => {
        // Only controls this function disables are re-enabled, so that a control the server drew
        // disabled stays disabled.
        if (busy) {
            box.querySelectorAll("button:enabled, input:enabled").forEach((control) => {
                control.disabled = true;
                control.dataset.busy = "";
            });
        } else {
            box.querySelectorAll("[data-busy]").forEach((control) => {
                control.disabled = false;
                delete control.dataset.busy;
            });
        }
        box.querySelector("[data-spinner]")?.remove();
        if (busy) {
            box.querySelector("[data-picture]").insertAdjacentHTML("beforeend", spinner);
        }
    };

    document.addEventListener("upload:start", (event) => {
        const box = event.target.closest("[data-picture-box]");
        if (!box) {
            return;
        }
        const message = box.querySelector("[data-message]");
        message.textContent = "";
        message.hidden = true;
        setBusy(box, true);
    });

    document.addEventListener("upload:end", (event) => {
        const box = event.target.closest("[data-picture-box]");
        if (!box) {
            return;
        }
        if (event.detail.ok) {
            const parent = box.parentElement;
            box.outerHTML = event.detail.html;
            htmx.process(parent);
            return;
        }
        const message = box.querySelector("[data-message]");
        message.textContent = event.detail.message;
        message.hidden = false;
        setBusy(box, false);
    });
}
