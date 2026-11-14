// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { forward, hourAngle, minuteAngle, secondAngle, ticks, timeText } from '../src/clockface.ts'
import { fitFontSize, MAX_SIZE, MIN_SIZE } from '../src/display.ts'

test('the hour hand moves with the minutes', () => {
  assert.equal(hourAngle(14, 5), 60 + 2.5)
  assert.equal(hourAngle(0, 0), 0)
  assert.equal(hourAngle(12, 30), 15)
  assert.equal(hourAngle(23, 59), 330 + 29.5)
  assert.equal(minuteAngle(5), 30)
  assert.equal(secondAngle(59), 354)
})

test('a hand only moves forward across the wrap', () => {
  assert.equal(forward(undefined, 30), 30)
  assert.equal(forward(354, 0), 360)
  assert.equal(forward(360, 6), 366)
  assert.equal(forward(366, 12), 372)
  assert.equal(forward(30, 36), 36)
})

test('twelve ticks, quarters heavier', () => {
  const t = ticks()
  assert.equal(t.length, 12)
  assert.deepEqual(t.filter((x) => x.quarter).map((x) => x.angle), [0, 90, 180, 270])
})

test('time text for the accessible name', () => {
  assert.equal(timeText(14, 5), '14:05')
  assert.equal(timeText(9, 5, 3), '09:05:03')
})

test('display text is scaled to fill the box', () => {
  // Text 200x100 at 100px in a 400x300 box: width limits, 200px.
  assert.equal(fitFontSize(400, 300, 200, 100, 100), 200)
  // A short box limits instead.
  assert.equal(fitFontSize(400, 50, 200, 100, 100), 50)
  assert.equal(fitFontSize(0, 50, 200, 100, 100), MIN_SIZE)
  assert.equal(fitFontSize(9000, 9000, 100, 100, 100), MAX_SIZE)
  assert.equal(fitFontSize(4, 4, 200, 100, 100), MIN_SIZE)
})
