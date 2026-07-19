(function () {
    function escapeHtml(value) {
        return String(value)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
    }

    function inlineMarkdown(value) {
        return escapeHtml(value)
            .replace(/`([^`]+)`/g, '<code>$1</code>')
            .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
            .replace(/\*([^*]+)\*/g, '<em>$1</em>');
    }

    function renderMarkdown(source) {
        var lines = source.split('\n');
        var html = '';
        var inList = false;

        for (var index = 0; index < lines.length; index += 1) {
            var line = lines[index];

            if (/^### /.test(line)) {
                if (inList) {
                    html += '</ul>';
                    inList = false;
                }
                html += '<h3>' + inlineMarkdown(line.slice(4)) + '</h3>';
            } else if (/^## /.test(line)) {
                if (inList) {
                    html += '</ul>';
                    inList = false;
                }
                html += '<h2>' + inlineMarkdown(line.slice(3)) + '</h2>';
            } else if (/^# /.test(line)) {
                if (inList) {
                    html += '</ul>';
                    inList = false;
                }
                html += '<h1>' + inlineMarkdown(line.slice(2)) + '</h1>';
            } else if (/^[-*] /.test(line)) {
                if (!inList) {
                    html += '<ul>';
                    inList = true;
                }
                html += '<li>' + inlineMarkdown(line.slice(2)) + '</li>';
            } else if (line.trim() === '') {
                if (inList) {
                    html += '</ul>';
                    inList = false;
                }
            } else {
                if (inList) {
                    html += '</ul>';
                    inList = false;
                }
                html += '<p>' + inlineMarkdown(line) + '</p>';
            }
        }

        if (inList) {
            html += '</ul>';
        }

        return html;
    }

    window.axiniteMarkdown = {
        escapeHtml: escapeHtml,
        inlineMarkdown: inlineMarkdown,
        renderMarkdown: renderMarkdown
    };
})();
