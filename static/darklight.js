// Modified from:
// https://github.com/argyleink/gui-challenges/blob/main/theme-switch/public/theme-toggle.js
// https://stackoverflow.com/a/19844757

const THEME_STORAGE = 'theme'
const THEME_LIGHT = 'light'
const THEME_DARK = 'dark'
const toggleTheme = () => {
    theme.value = theme.value === THEME_LIGHT ? THEME_DARK : THEME_LIGHT
    storeTheme(theme.value)
}
const preferredTheme = () => {
    return localStorage.getItem(THEME_STORAGE) ? localStorage.getItem(THEME_STORAGE) : (window.matchMedia('(prefers-color-scheme: dark)').matches ? THEME_DARK : THEME_LIGHT)
}
const storeTheme = (value) => {
    value = value ? value : theme.value;
    localStorage.setItem(THEME_STORAGE, value)
    applyTheme(value)
}
const clearTheme = () => {
    localStorage.clear(THEME_STORAGE)
}
const applyTheme = (value) => {
    value = value ? value : theme.value;
    document.firstElementChild.setAttribute('data-theme', value)
    themeCallbacks.forEach(f => {
        if (f && typeof(f) === 'function') f.call(null, value);
    })
}
const themeCallbacks = []
const theme = { value: preferredTheme(), }
applyTheme()
window.addEventListener('load', applyTheme, false);
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', ({matches:isDark}) => {
    theme.value = isDark ? THEME_DARK : THEME_LIGHT
    storeTheme()
})
