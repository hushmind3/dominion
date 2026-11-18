import assert from 'node:assert/strict'
import {test} from 'node:test'
import {isFreeModel} from '../src/utils/modelFree.ts'

test('recognizes explicit provider free metadata', () => {
  assert.equal(isFreeModel({id: 'provider/model', free: true}), true)
  assert.equal(isFreeModel({id: 'provider/model', freeStatus: 'free'}), true)
})

test('recognizes OpenRouter :free suffix even without catalog metadata', () => {
  assert.equal(isFreeModel({id: 'meta/llama-3:free'}), true)
  assert.equal(isFreeModel({id: 'qwen/qwen3:FREE', free: false, freeStatus: 'unknown'}), true)
  assert.equal(isFreeModel({id: 'google/gemma:free ', free: null}), true)
})

test('does not treat unknown, promotional or paid models as free', () => {
  assert.equal(isFreeModel({id: 'provider/paid-model'}), false)
  assert.equal(isFreeModel({id: 'provider/free-trial'}), false)
  assert.equal(isFreeModel({id: 'provider/model:free-tier'}), false)
  assert.equal(isFreeModel({id: 'provider/model', freeStatus: 'unknown'}), false)
  assert.equal(isFreeModel({id: 'provider/model:free', freeStatus: 'paid'}), false)
  assert.equal(isFreeModel({id: 'provider/model', free: false, freeStatus: 'paid'}), false)
})
