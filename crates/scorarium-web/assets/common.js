const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function applyTheme() {
    document.documentElement.setAttribute("data-bs-theme", darkQuery.matches ? "dark" : "light");
}

applyTheme();
darkQuery.addEventListener("change", applyTheme);
