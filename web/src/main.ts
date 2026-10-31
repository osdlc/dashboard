import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import AmbientApp from './AmbientApp.vue'
import { ambient } from './mode'

createApp(ambient ? AmbientApp : App).mount('#app')
