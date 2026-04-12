import { describe, it, expect } from '@jest/globals';
import { MemoryStorage } from './memory.js';

describe('MemoryStorage', () => {
  it('put → get roundtrip', async () => {
    const s   = new MemoryStorage();
    const data = new TextEncoder().encode('hello world');
    const id   = await s.put(data);
    const back = await s.get(id);
    expect(back).toEqual(data);
  });

  it('ids are unique', async () => {
    const s  = new MemoryStorage();
    const id1 = await s.put(new Uint8Array([1]));
    const id2 = await s.put(new Uint8Array([2]));
    expect(id1).not.toBe(id2);
  });

  it('delete removes blob', async () => {
    const s  = new MemoryStorage();
    const id = await s.put(new Uint8Array([1, 2, 3]));
    await s.delete(id);
    await expect(s.get(id)).rejects.toThrow();
  });

  it('missing blob throws', async () => {
    const s = new MemoryStorage();
    await expect(s.get('nonexistent')).rejects.toThrow();
  });
});
