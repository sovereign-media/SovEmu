// A headless Chromium playing one `sovemu-worker serve` session (sovereign-media/SovEmu#282):
// it offers receive-only video and audio plus the `input` channel, as the web client does, then
// presses buttons on a schedule so the game moves, and closes after the given seconds.
//
//   node client.mjs http://127.0.0.1:8090 SECONDS 'FRAME:BUTTONS,...'
//
// Buttons are libretro joypad names (b y select start up down left right a x l r); a press is
// held for 5 frames (at 60 per second). Needs SovEmu.Web's Playwright (npm ci there).
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const here = path.dirname(fileURLToPath(import.meta.url))
const require = createRequire(path.join(here, '../../SovEmu.Web/package.json'))
const { chromium } = require('@playwright/test')

const [base, seconds = '60', presses = ''] = process.argv.slice(2)
const browser = await chromium.launch()
const page = await browser.newPage()
page.on('console', (m) => console.error(`page: ${m.text()}`))
await page.goto(`${base}/`)
const stats = await page.evaluate(
  async ({ seconds, presses }) => {
    const ids = { b: 0, y: 1, select: 2, start: 3, up: 4, down: 5, left: 6, right: 7, a: 8, x: 9, l: 10, r: 11 }
    const pc = new RTCPeerConnection()
    pc.addTransceiver('video', { direction: 'recvonly' })
    pc.addTransceiver('audio', { direction: 'recvonly' })
    const input = pc.createDataChannel('input', { ordered: false, maxRetransmits: 0 })
    let frames = 0
    pc.ontrack = (e) => {
      if (e.track.kind !== 'video') return
      const video = document.createElement('video')
      video.muted = true
      video.srcObject = new MediaStream([e.track])
      document.body.append(video)
      video.play()
      const count = () => {
        frames++
        video.requestVideoFrameCallback(count)
      }
      video.requestVideoFrameCallback(count)
    }
    await pc.setLocalDescription(await pc.createOffer())
    await new Promise((r) => (pc.iceGatheringState === 'complete' ? r() : (pc.onicegatheringstatechange = () => pc.iceGatheringState === 'complete' && r())))
    const answer = await fetch('/api/offer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(pc.localDescription),
    }).then((r) => r.json())
    await pc.setRemoteDescription(answer)
    await new Promise((r) => (input.readyState === 'open' ? r() : (input.onopen = r)))
    const schedule = presses
      .split(',')
      .filter(Boolean)
      .map((p) => {
        const [frame, buttons] = p.split(':')
        return { at: Number(frame), mask: buttons.split('+').reduce((m, b) => m | (1 << ids[b]), 0) }
      })
    let seq = 0
    const start = performance.now()
    while (performance.now() - start < seconds * 1000) {
      const frame = Math.floor(((performance.now() - start) / 1000) * 60)
      const mask = schedule.filter((s) => frame >= s.at && frame < s.at + 5).reduce((m, s) => m | s.mask, 0)
      const packet = new DataView(new ArrayBuffer(13))
      packet.setUint8(0, 0x01)
      packet.setUint8(1, 1)
      packet.setUint32(2, seq++, true)
      packet.setUint32(6, Math.floor(performance.now()) >>> 0, true)
      packet.setUint8(10, 1)
      packet.setUint16(11, mask, true)
      if (input.readyState === 'open') input.send(packet.buffer)
      await new Promise((r) => setTimeout(r, 16))
    }
    pc.close()
    return { frames, connection: pc.connectionState }
  },
  { seconds: Number(seconds), presses },
)
console.log(JSON.stringify(stats))
await browser.close()
