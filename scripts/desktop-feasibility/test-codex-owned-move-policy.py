import importlib.util,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'canary/actions'))
spec=importlib.util.spec_from_file_location('private_move_qualification',Path(__file__).with_name('run-qualification.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
p=Path('/tmp')
s=dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='macOS',NANH_CODEX_PUBLIC_ONBOARDING='engineering',NANH_CODEX_PROJECT_POLICY='open-project',NANH_CODEX_OWNED_MOVE='source-point')
r=m.qualification_environment('chatgpt-desktop',p,p,p,s);assert r['NANH_CODEX_OWNED_MOVE']=='source-point';assert 'NANH_CODEX_PROJECT_ARTIFACT_SHA256' not in r
for change in [{'RUNNER_OS':'Linux'},{'NANH_CODEX_OWNED_MOVE':'unknown'},{'NANH_CODEX_PROJECT_POLICY':None},{'NANH_CODEX_PUBLIC_ONBOARDING':None},{'NANH_DESKTOP_QUALIFICATION_MODE':'startup-baseline'},{'RUNNER_ENVIRONMENT':'self-hosted'}]:
 try:m.qualification_environment('chatgpt-desktop',p,p,p,s|change)
 except ValueError:pass
 else:raise AssertionError(change)
for app in ['hermes-desktop','zed-desktop','pen-desktop','claude-desktop']:
 try:m.qualification_environment(app,p,p,p,s)
 except ValueError:pass
 else:raise AssertionError(app)
print('11 source policy and derived-pin privacy cases passed')
