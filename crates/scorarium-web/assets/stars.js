function attachStars(container) {
    const radios = [...container.querySelectorAll("input[type=radio]")];
    const labels = radios.map((radio) => container.querySelector(`label[for="${radio.id}"]`));

    function paint(n) {
        labels.forEach((label, i) => {
            const icon = label.querySelector("i");
            icon.classList.toggle("bi-star-fill", i < n);
            icon.classList.toggle("bi-star", i >= n);
        });
    }

    const checked = () => radios.findIndex((radio) => radio.checked) + 1;

    labels.forEach((label, i) => {
        label.addEventListener("mouseenter", () => paint(i + 1));
        label.addEventListener("click", (event) => {
            // A radio cannot be unchecked by clicking it, so clearing is done here.
            if (radios[i].checked) {
                event.preventDefault();
                radios[i].checked = false;
                paint(0);
            }
        });
    });
    radios.forEach((radio, i) => radio.addEventListener("change", () => paint(i + 1)));
    container.addEventListener("mouseleave", () => paint(checked()));
}

document.querySelectorAll("[data-stars]").forEach(attachStars);
