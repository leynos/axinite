tailwind.config = {
    darkMode: 'class',
    theme: {
        extend: {
            fontFamily: {
                sans: ['Molengo', 'sans-serif'],
                serif: ['Buenard', 'serif'],
                mono: ['ui-monospace', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', "Liberation Mono", "Courier New", 'monospace'],
            },
            colors: {
                glass: {
                    100: 'rgba(255, 255, 255, 0.03)',
                    200: 'rgba(255, 255, 255, 0.05)',
                    300: 'rgba(255, 255, 255, 0.1)',
                    border: 'rgba(255, 255, 255, 0.08)',
                },
                brand: {
                    emerald: '#00ff41',
                    indigo: '#4f46e5',
                    dark: '#09090b',
                }
            },
            backgroundImage: {
                'mesh-gradient': 'radial-gradient(circle at 15% 50%, rgba(0, 255, 65, 0.08), transparent 25%), radial-gradient(circle at 85% 30%, rgba(79, 70, 229, 0.08), transparent 25%)',
            }
        }
    }
}
