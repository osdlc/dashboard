import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import AmbientApp from './AmbientApp.vue'
import DemoApp from './DemoApp.vue'
import { ambient, demo } from './mode'
import { applyColorTheme, DEFAULT_THEME } from './colors'

// Colors before the first paint; the server's settings may then pick another theme.
applyColorTheme(DEFAULT_THEME)

createApp(demo ? DemoApp : ambient ? AmbientApp : App).mount('#app')
