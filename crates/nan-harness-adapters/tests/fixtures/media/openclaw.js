let imageProvider;
plugin.register({
  registerSpeechProvider() {},
  registerMediaUnderstandingProvider() {},
  registerImageGenerationProvider(provider) { imageProvider = provider; }
});
const expectedDefault = process.argv[2];
if (imageProvider.defaultModel !== expectedDefault) throw new Error('default model lost');
if (imageProvider.models.join(',') !== 'flux-2-klein,qwen-image-2.1') throw new Error('model catalog missing');
for (const model of [undefined, 'flux-2-klein', 'qwen-image-2.1']) {
  const result = await imageProvider.generateImage({ prompt: 'synthetic', model });
  const args = mediaCalls.at(-1);
  if (args[args.indexOf('--model') + 1] !== (model ?? expectedDefault)) throw new Error('model override lost');
  if (result.images[0].buffer.toString() !== 'synthetic-image') throw new Error('output lost');
}
for (const reference of [Buffer.from('reference'), {buffer: Buffer.from('reference')}]) {
  await imageProvider.generateImage({prompt: 'edit', model: 'flux-2-klein', inputImages: [reference]});
}
if (!mediaCalls.at(-1).includes('--input-image')) throw new Error('reference lost');
const count = mediaCalls.length;
for (const request of [
  {prompt: 'bad model', model: 'qwen3.6'},
  {prompt: 'unsupported edit', model: 'qwen-image-2.1', inputImages: [Buffer.from('reference')]}
]) {
  let rejected = false;
  try { await imageProvider.generateImage(request); } catch { rejected = true; }
  if (!rejected || mediaCalls.length !== count) throw new Error('invalid request reached provider');
}
