{
    const spinner = `
        <div class="position-absolute top-0 start-0 w-100 h-100 d-flex align-items-center justify-content-center bg-body bg-opacity-75" data-spinner>
            <div class="spinner-border" role="status"><span class="visually-hidden">Working</span></div>
        </div>`;

    const showPicture = (root, templateSelector, src) => {
        const picture = root.querySelector(templateSelector).content.cloneNode(true);
        if (src) {
            picture.querySelector("a")?.setAttribute("href", src);
            picture.querySelector("img").setAttribute("src", src);
        }
        root.querySelector("[data-picture]").replaceChildren(picture);
    };

    document.addEventListener("upload:start", (event) => {
        const root = event.target.closest("[data-picture-input]");
        if (!root) {
            return;
        }
        root.querySelector("[data-spinner]")?.remove();
        root.querySelector("[data-picture]").insertAdjacentHTML("beforeend", spinner);
    });

    document.addEventListener("upload:end", (event) => {
        const root = event.target.closest("[data-picture-input]");
        if (!root) {
            return;
        }
        root.querySelector("[data-spinner]")?.remove();
        if (!event.detail.ok) {
            return;
        }
        const { id } = JSON.parse(event.detail.body);
        root.querySelector("input[name=remove_picture]").disabled = true;
        showPicture(root, "[data-staged-picture]", `/api/stage/${encodeURIComponent(id)}`);
        root.querySelector("[data-file-clear]").hidden = false;
    });

    document.addEventListener("click", (event) => {
        const remove = event.target.closest("[data-picture-input] [data-file-clear]");
        if (!remove) {
            return;
        }
        const root = remove.closest("[data-picture-input]");
        root.querySelector("input[name=remove_picture]").disabled = false;
        showPicture(root, "[data-placeholder-picture]");
        remove.hidden = true;
    });
}
