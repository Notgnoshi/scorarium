{
    const inputs = (item) => item.querySelectorAll('input[name^="proposal["]');

    const setState = (item, state) => {
        item.dataset.candidate = state;
        inputs(item).forEach((input) => (input.value = state));
    };

    const fill = (item) => {
        const filled = [];
        for (const attribute of item.attributes) {
            if (!attribute.name.startsWith("data-fill-")) {
                continue;
            }
            const input = item.closest("form").elements[attribute.name.slice("data-fill-".length)];
            if (input) {
                input.value = attribute.value;
                input.dispatchEvent(new Event("change", { bubbles: true }));
                filled.push(input);
            }
        }
        return filled;
    };

    const visibleButton = (item) =>
        [...item.querySelectorAll("[data-proposal-state]")].find((button) => button.offsetParent !== null);

    const countRemaining = () => {
        const toggle = document.querySelector("[data-reject-remaining]");
        if (!toggle) {
            return;
        }
        const count = document.querySelectorAll('#edit [data-candidate="pending"]').length;
        toggle.querySelector("label").textContent =
            `Reject ${count} remaining ${count === 1 ? "proposal" : "proposals"}`;
        toggle.hidden = count === 0;
        if (count === 0) {
            toggle.querySelector("input").checked = false;
        }
    };

    document.addEventListener("click", (event) => {
        const button = event.target.closest("[data-proposal-state]");
        const item = button?.closest("[data-candidate]");
        if (!item) {
            return;
        }
        const state = button.dataset.proposalState;
        setState(item, state);
        if (state === "accepted") {
            const filled = fill(item);
            for (const sibling of item.parentElement.children) {
                if (sibling !== item && sibling.dataset.candidate !== "accepted") {
                    setState(sibling, "rejected");
                }
            }
            const input = filled[0];
            if (input?.matches("[data-typeahead]")) {
                // Suggestions would cover the other candidates and could replace the accepted value.
                input.dataset.typeaheadQuiet = "";
            }
            input?.focus();
        } else if (state === "rejected") {
            visibleButton(item)?.focus();
        } else {
            item.querySelector('[data-proposal-state="accepted"]').focus();
        }
        countRemaining();
    });

    countRemaining();
}
