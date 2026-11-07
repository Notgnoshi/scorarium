const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function applyTheme() {
    document.documentElement.setAttribute("data-bs-theme", darkQuery.matches ? "dark" : "light");
}

applyTheme();
darkQuery.addEventListener("change", applyTheme);

// Delegated on the document so the listener works from <head> and covers every form that opts in.
document.addEventListener("submit", (event) => {
    const message = event.target.dataset.confirm;
    if (message && !confirm(message)) {
        event.preventDefault();
    }
});
