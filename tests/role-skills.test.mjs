import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { roleConfiguration } from '../src/role-skills.js'
import { addAgent } from '../src/roster.js'

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'cf-role-skills-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  return { root, env: { HOME: root, CONSENSFLOW_HOME: join(root, 'app') } }
}

test('lead skills use a private directory and leave global canaries unchanged', async (t) => {
  const { root, env } = await fixture(t)
  const global = join(root, '.claude', 'skills', 'consensflow')
  await mkdir(global, { recursive: true })
  await writeFile(join(global, 'SKILL.md'), 'global canary')
  const configuration = await roleConfiguration('claude-code', { role: 'lead', env })
  assert.equal(configuration.args[0], '--add-dir')
  assert.ok(configuration.args[1].startsWith(env.CONSENSFLOW_HOME))
  const text = await readFile(
    join(configuration.args[1], '.claude/skills/consensflow-lead/SKILL.md'),
    'utf8',
  )
  assert.match(text, /name: consensflow-lead/)
  assert.doesNotMatch(text, /name: consensflow-pm/)
  assert.equal(await readFile(join(global, 'SKILL.md'), 'utf8'), 'global canary')
})

test('workers get no role skill and Pi loads Markdown without extension code', async (t) => {
  const { env } = await fixture(t)
  assert.deepEqual(await roleConfiguration('pi', { role: 'worker', env }), { args: [], env: {} })
  const configuration = await roleConfiguration('pi', { role: 'lead', env })
  assert.equal(configuration.args[0], '--skill')
  assert.match(configuration.args[1], /SKILL.md$/)
  assert.equal(configuration.args.length, 4)
  assert.deepEqual(configuration.env, {})
})

test('OpenCode preserves user configuration and adds the role skill path', async (t) => {
  const { env } = await fixture(t)
  env.OPENCODE_CONFIG_CONTENT = JSON.stringify({
    theme: 'user',
    skills: { paths: ['/user/skills'] },
  })
  const configuration = await roleConfiguration('opencode', { role: 'lead', env })
  const merged = JSON.parse(configuration.env.OPENCODE_CONFIG_CONTENT)
  assert.equal(merged.theme, 'user')
  assert.equal(merged.skills.paths[0], '/user/skills')
  assert.equal(merged.skills.paths.length, 2)
})

test('Codex appends to native effective instructions using only configuration reads', async (t) => {
  const { root, env } = await fixture(t)
  const executable = join(root, 'codex')
  await writeFile(
    executable,
    `#!${process.execPath}\n
import { createInterface } from 'node:readline';
const lines = createInterface({input: process.stdin});
lines.on('line', line => {
  const request = JSON.parse(line);
  if (request.method === 'initialize') console.log(JSON.stringify({id:request.id,result:{}}));
  else if (request.method === 'config/read') console.log(JSON.stringify({id:request.id,result:{config:{developer_instructions:'existing user instructions'}}}));
  else if (request.method !== 'initialized') process.exit(20);
});
`,
  )
  await chmod(executable, 0o755)
  const configuration = await roleConfiguration('codex', {
    role: 'lead',
    env,
    executable,
    cwd: root,
  })
  assert.equal(configuration.args[0], '-c')
  assert.match(configuration.args[1], /existing user instructions/)
  assert.match(configuration.args[1], /consensflow-lead/)
  assert.doesNotMatch(configuration.args[1], /consensflow-pm/)
})

test('both roles enter native startup context without a skill invocation', async (t) => {
  for (const kind of ['claude-code', 'codex', 'opencode', 'pi', 'devin']) {
    for (const role of ['lead', 'pm', 'advisor']) {
      await t.test(`${kind} ${role}`, async (t) => {
        const { env } = await fixture(t)
        addAgent(
          { name: 'saved-worker', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
          env,
        )
        const existing = 'User instructions: preserve "quotes", `backticks`, $HOME\nand newlines.'
        const original = {
          theme: 'user',
          skills: { paths: ['/user/skills'], urls: ['https://example.com/skills'] },
          instructions: ['/user/rules.md'],
        }
        env.OPENCODE_CONFIG_CONTENT = JSON.stringify(original)
        const configuration = await roleConfiguration(kind, {
          role,
          env,
          readInstructions: async () => existing,
        })
        const file = join(
          env.CONSENSFLOW_HOME,
          'roles',
          role,
          '.claude',
          'skills',
          `consensflow-${role}`,
          'SKILL.md',
        )
        const content = await readFile(file, 'utf8')
        let instructions
        if (kind === 'claude-code') {
          const index = configuration.args.indexOf('--append-system-prompt-file')
          assert.notEqual(index, -1, 'the full role must enter the native system prompt')
          instructions = await readFile(configuration.args[index + 1], 'utf8')
          assert.equal(
            configuration.args[configuration.args.indexOf('--system-prompt-snapshot') + 1],
            'off',
            'resumed conversations must use the current role instructions',
          )
        } else if (kind === 'pi') {
          const index = configuration.args.indexOf('--append-system-prompt')
          assert.notEqual(index, -1, '--skill alone only advertises the role')
          instructions = configuration.args[index + 1]
        } else if (kind === 'opencode') {
          const merged = JSON.parse(configuration.env.OPENCODE_CONFIG_CONTENT)
          assert.deepEqual(merged.instructions, [...original.instructions, file])
          assert.equal(merged.theme, original.theme)
          assert.deepEqual(merged.skills.urls, original.skills.urls)
          assert.equal(merged.skills.paths[0], original.skills.paths[0])
          instructions = await readFile(merged.instructions.at(-1), 'utf8')
          const repeated = await roleConfiguration(kind, {
            role,
            env: { ...env, ...configuration.env },
          })
          assert.deepEqual(JSON.parse(repeated.env.OPENCODE_CONFIG_CONTENT), merged)
        } else if (kind === 'devin') {
          instructions = await readFile(configuration.env.CF_DEVIN_ROLE_FILE, 'utf8')
        } else {
          instructions = JSON.parse(configuration.args[1].slice('developer_instructions='.length))
          assert.ok(instructions.startsWith(`${existing}\n\n`))
        }
        assert.ok(
          instructions.includes(content),
          'all role instructions, including the body, must already be loaded',
        )
        assert.ok(content.includes(role === 'pm' ? '# ConsensFlow PM' : `# ConsensFlow ${role}`))
        assert.ok(!instructions.includes(`name: consensflow-${role === 'pm' ? 'lead' : 'pm'}`))
        if (role !== 'advisor') {
          assert.match(instructions, /\| saved-worker \|/)
          assert.match(instructions, /Complex debugging, architecture and detailed review/)
          assert.match(instructions, /cross-model review/i)
        } else {
          assert.doesNotMatch(instructions, /\| saved-worker \|/)
        }
        assert.equal(env.OPENCODE_CONFIG_CONTENT, JSON.stringify(original))
      })
    }
  }
})

test('OpenCode rejects malformed instruction lists before native launch', async (t) => {
  for (const instructions of ['rules.md', null, {}, 7, ['rules.md', 7]]) {
    await t.test(JSON.stringify(instructions), async (t) => {
      const { env } = await fixture(t)
      env.OPENCODE_CONFIG_CONTENT = JSON.stringify({ instructions })
      await assert.rejects(
        roleConfiguration('opencode', { role: 'pm', env }),
        /OpenCode instructions must be an array of paths/,
      )
    })
  }
})

test('workers receive no role instructions across all harnesses', async (t) => {
  const { env } = await fixture(t)
  for (const kind of ['claude-code', 'codex', 'opencode', 'pi', 'kimi']) {
    assert.deepEqual(
      await roleConfiguration(kind, {
        role: 'worker',
        env,
        readInstructions: async () => assert.fail('workers must not resolve role instructions'),
      }),
      { args: [], env: {} },
    )
  }
  await assert.rejects(readFile(join(env.CONSENSFLOW_HOME, 'roles')), { code: 'ENOENT' })
})

test('advisors can research and run existing tests but only the PM writes specifications', async (t) => {
  const { env } = await fixture(t)
  const configuration = await roleConfiguration('pi', { role: 'advisor', env })
  const instructions = configuration.args.at(-1)
  assert.match(instructions, /search the web/)
  assert.match(instructions, /Run existing tests/)
  assert.match(instructions, /Only the PM writes or revises specifications/)
  assert.match(instructions, /Do not edit/)
  assert.match(instructions, /owning PM/)
  assert.match(instructions, /Do not delegate/)
})

test('launch reads the current roster for its own role and avoids unchanged writes', async (t) => {
  const { env } = await fixture(t)
  const { editAgent } = await import('../src/roster.js')
  const { stat, utimes } = await import('node:fs/promises')
  addAgent({ name: 'example', harness: 'codex', model: 'gpt-6-astra', effort: 'max' }, env)
  for (const role of ['lead', 'pm']) {
    const first = await roleConfiguration('pi', { role, env })
    const file = first.args[1]
    const initial = first.args.at(-1)
    assert.ok(initial.includes('| example |'))
    await utimes(file, 1, 1)
    const timestamp = (await stat(file)).mtimeMs
    assert.deepEqual(await roleConfiguration('pi', { role, env }), first)
    assert.equal((await stat(file)).mtimeMs, timestamp, 'unchanged role is not rewritten')
    editAgent('example', { description: `Updated for ${role}` }, env)
    const refreshed = await roleConfiguration('pi', { role, env })
    assert.ok(refreshed.args.at(-1).includes(`Updated for ${role}`))
    assert.equal(await readFile(file, 'utf8'), refreshed.args.at(-1))
    assert.equal((await stat(file)).mode & 0o777, 0o600)
  }
  await assert.rejects(readFile(join(env.CONSENSFLOW_HOME, 'skills-manifest.json')), {
    code: 'ENOENT',
  })
})
