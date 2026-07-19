/**
 * Shared helpers for the Jobs and Routines dashboard pages.
 */

function escapeHtml(value) {
    return String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

/**
 * Render summary cards into a container element.
 *
 * @param {HTMLElement} containerEl  – target grid container
 * @param {Array}       defs        – [{ key, label, color }]
 * @param {Function}    countFn     – (key) => number
 * @param {string|null} activeFilter
 * @param {Function}    onToggle    – (key) => void, called when a card is clicked
 */
function renderSummaryCards(containerEl, defs, countFn, activeFilter, onToggle) {
    containerEl.innerHTML = '';
    defs.forEach(function (def) {
        var count = countFn(def.key);
        var isActive = activeFilter === def.key;
        var card = document.createElement('button');
        var countEl = document.createElement('div');
        var labelEl = document.createElement('div');
        card.type = 'button';
        card.setAttribute('aria-pressed', String(isActive));
        card.className = 'bg-glass-100 border rounded-xl p-4 text-center cursor-pointer transition-colors hover:bg-glass-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-emerald/60 ' +
            (isActive ? 'border-brand-emerald/40' : 'border-glass-border');
        countEl.className = 'text-3xl font-bold ' + def.color;
        countEl.textContent = String(count);
        labelEl.className = 'text-[10px] font-semibold text-zinc-500 uppercase tracking-wider mt-1';
        labelEl.textContent = def.label;
        card.appendChild(countEl);
        card.appendChild(labelEl);
        card.addEventListener('click', function () {
            onToggle(def.key);
        });
        containerEl.appendChild(card);
    });
}

/**
 * Wire up the detail modal's close button, backdrop, and Escape key.
 *
 * Expects element ids: detail-modal, detail-close, detail-backdrop.
 * Returns { open, close } functions that show/hide the modal.
 */
function initDetailModal() {
    var modal = document.getElementById('detail-modal');
    var closeBtn = document.getElementById('detail-close');
    var backdrop = document.getElementById('detail-backdrop');
    var appShell = document.getElementById('app-shell');
    var focusableSelector = 'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';
    var lastFocusedElement = null;
    var modalKeydownHandler = null;

    if (!modal) {
        return {
            open: function () {},
            close: function () {}
        };
    }

    function isOpen() {
        return !modal.classList.contains('hidden');
    }

    function setBackgroundInert(isInert) {
        if (!appShell) {
            return;
        }

        appShell.inert = isInert;
        if (isInert) {
            appShell.setAttribute('aria-hidden', 'true');
        } else {
            appShell.removeAttribute('aria-hidden');
        }
    }

    function getFocusableInModal() {
        return Array.prototype.filter.call(
            modal.querySelectorAll(focusableSelector),
            function (el) {
                return !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true' && el.offsetParent !== null;
            }
        );
    }

    function focusModalStart() {
        var focusable = getFocusableInModal();
        var target = focusable[0] || modal;
        target.focus();
    }

    function handleModalKeydown(e) {
        var focusable;
        var first;
        var last;

        if (!isOpen()) {
            return;
        }

        if (e.key !== 'Tab') {
            return;
        }

        focusable = getFocusableInModal();
        first = focusable[0];
        last = focusable[focusable.length - 1];

        if (!first || !last) {
            e.preventDefault();
            modal.focus();
            return;
        }

        if (e.shiftKey && document.activeElement === first) {
            e.preventDefault();
            last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
            e.preventDefault();
            first.focus();
        }
    }

    function close() {
        if (!isOpen()) {
            return;
        }

        setBackgroundInert(false);
        modal.setAttribute('aria-hidden', 'true');
        if (modalKeydownHandler) {
            document.removeEventListener('keydown', modalKeydownHandler);
            modalKeydownHandler = null;
        }
        if (lastFocusedElement && typeof lastFocusedElement.focus === 'function' && lastFocusedElement.isConnected) {
            lastFocusedElement.focus();
        }
        lastFocusedElement = null;
        modal.classList.add('hidden');
    }

    function open() {
        if (isOpen()) {
            return;
        }

        lastFocusedElement = document.activeElement;
        setBackgroundInert(true);
        modal.classList.remove('hidden');
        modal.setAttribute('aria-hidden', 'false');
        if (modalKeydownHandler) {
            document.removeEventListener('keydown', modalKeydownHandler);
        }
        modalKeydownHandler = handleModalKeydown;
        document.addEventListener('keydown', modalKeydownHandler);
        focusModalStart();
    }

    if (closeBtn) {
        closeBtn.addEventListener('click', close);
    }
    if (backdrop) {
        backdrop.addEventListener('mousedown', function (e) {
            e.preventDefault();
        });
        backdrop.addEventListener('click', close);
    }
    document.addEventListener('keydown', function (e) {
        if (e.key === 'Escape' && isOpen()) close();
    });

    return { open: open, close: close };
}
