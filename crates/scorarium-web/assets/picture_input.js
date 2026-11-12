{
    const spinner = `
        <div class="position-absolute top-0 start-0 w-100 h-100 d-flex align-items-center justify-content-center bg-body bg-opacity-75" data-spinner>
            <div class="spinner-border" role="status"><span class="visually-hidden">Working</span></div>
        </div>`;

    const frameOf = (event) => event.target.closest("[data-picture-input]")?.querySelector("[data-picture-frame]");
    const removal = (frame) => frame.parentElement.querySelector("input[name=remove_picture]");

    document.addEventListener("upload:start", (event) => {
        const frame = frameOf(event);
        if (!frame) {
            return;
        }
        frame.querySelector("[data-spinner]")?.remove();
        frame.insertAdjacentHTML("beforeend", spinner);
    });

    document.addEventListener("upload:end", (event) => {
        const frame = frameOf(event);
        if (!frame) {
            return;
        }
        frame.querySelector("[data-spinner]")?.remove();
        if (!event.detail.ok) {
            return;
        }
        const { id } = JSON.parse(event.detail.body);
        frame.querySelector("img").src = `/api/stage/${encodeURIComponent(id)}`;
        frame.dataset.hasPicture = "";
        removal(frame).disabled = true;
    });

    document.addEventListener("click", (event) => {
        const frame = event.target.closest("[data-picture-frame]");
        if (!frame) {
            return;
        }
        if (event.target.closest("[data-file-clear]")) {
            delete frame.dataset.hasPicture;
            frame.querySelector("img").removeAttribute("src");
            removal(frame).disabled = false;
        } else {
            frame.parentElement.querySelector("[data-browse]").click();
        }
    });
}
