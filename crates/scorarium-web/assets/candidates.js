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

    const acceptInto = (list, item) => {
        list.querySelector("[data-list-add]").click();
        const entry = [...list.querySelectorAll(":scope > [data-list-entry]")].at(-1);
        for (const attribute of item.attributes) {
            if (attribute.name.startsWith("data-pick-draft-")) {
                const noun = attribute.name.slice("data-pick-draft-".length);
                const icon = entry.querySelector(`[data-pick-icon="${CSS.escape(noun)}"]`);
                if (icon) {
                    icon.dataset.pickDraft = attribute.value;
                }
            }
        }
        for (const attribute of item.attributes) {
            if (!attribute.name.startsWith("data-fill-")) {
                continue;
            }
            const field = attribute.name.slice("data-fill-".length);
            const input = entry.querySelector(`[name$="[${field}]"]`);
            if (input) {
                input.value = attribute.value;
                input.dispatchEvent(new Event("change", { bubbles: true }));
            }
        }
        const sources = item.querySelector("[data-sources]").cloneNode(true);
        sources.classList.add("small", "align-self-center");
        entry.querySelector("[data-list-remove]").before(sources);
        const prefix = entry.querySelector("[name]").name.match(/^\w+\[[^\]]+\]/)[0];
        for (const proposal of inputs(item)) {
            const input = document.createElement("input");
            input.type = "hidden";
            input.name = `${prefix}[proposal]`;
            input.value = proposal.name.slice("proposal[".length, -1);
            entry.append(input);
        }
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
        const list = item.closest("[data-list]");
        if (state === "accepted" && list) {
            acceptInto(list, item);
        } else if (state === "accepted") {
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

    const edit = document.querySelector("form#edit[data-candidates-url]");

    const merge = (answer) => {
        const onPage = new Map([...inputs(edit)].map((input) => [input.name, input]));
        for (const candidate of answer.querySelectorAll("[data-candidate]")) {
            const proposals = [...inputs(candidate)];
            const held = proposals.find((input) => onPage.has(input.name));
            const missing = proposals.filter((input) => !onPage.has(input.name));
            if (!held) {
                const name = candidate.closest("[data-candidates]").dataset.candidates;
                const list = edit.querySelector(`[data-candidates="${CSS.escape(name)}"]`);
                const end = list?.querySelector(":scope > [data-candidates-end]");
                if (end) {
                    end.before(candidate);
                } else {
                    list?.append(candidate);
                }
                continue;
            }
            if (missing.length === 0) {
                continue;
            }
            const item = onPage.get(held.name).closest("[data-candidate]");
            for (const input of missing) {
                input.value = item.dataset.candidate;
                item.append(input);
            }
            item.querySelector("[data-sources]").textContent =
                candidate.querySelector("[data-sources]").textContent;
        }
        countRemaining();
    };

    const poll = async () => {
        try {
            const response = await fetch(edit.dataset.candidatesUrl, {
                headers: { "HX-Current-URL": location.href },
            });
            if (response.ok) {
                merge(new DOMParser().parseFromString(await response.text(), "text/html"));
            }
        } catch {
            // A failed poll is retried by the next one.
        }
        setTimeout(poll, 5000);
    };

    if (edit) {
        setTimeout(poll, 5000);
    }

    countRemaining();
}
