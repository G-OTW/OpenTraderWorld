# Primeiros passos

Você [instalou](/pt/guide/install) o OpenTraderWorld e tem suas credenciais de administrador. Veja como deixá-lo do seu jeito.

## Entrar

Abra o app e faça login em `/login`. Se sua senha foi **gerada pelo instalador**, você será solicitado a **escolher uma nova no primeiro login**, já que a senha gerada só funciona uma vez.

Você pode alterar seu nome de usuário ou senha a qualquer momento em **Configurações → Conta**. Alterar a senha encerra todas as suas sessões. Perdeu o acesso? Não há e-mail de redefinição: recupere pelo shell do host, veja [Esqueci minha senha](/pt/guide/troubleshooting#forgot-password).

Em seguida, em **Configurações → Segurança**: ative a **autenticação em dois fatores** e verifique quais navegadores estão conectados. Faça isso antes de deixar qualquer coisa além desta máquina alcançar o app. Veja [Segurança da conta](/pt/config/security).

## Defina seus padrões

Vá em **Configurações → Padrões** e escolha:

- **Idioma**: aplica-se a todo o app imediatamente (inglês, francês, alemão, espanhol, italiano, português, chinês).
- **Moeda padrão** e **fuso horário**: usados como valores iniciais em todos os módulos.

## Instale seus módulos

Abra **Configurações → Módulos**. Todo módulo já vem com o app; instalar um apenas o torna disponível no seletor de módulos e no dashboard, e nada é baixado.

- **Instale** os módulos que você quer. Comece pequeno, você pode adicionar mais a qualquer momento.
- Alguns módulos dependem de outros: **Historical Data Visualization**, **Backtest** e **Quant Tools** precisam todos de **Historical Data** (eles trabalham sobre seus conjuntos de dados baixados).
- **Desconectar** oculta um módulo e o torna inacessível; seus dados são mantidos, a menos que você também marque *excluir dados*. Você pode reinstalar a qualquer momento.

Não sabe por onde começar? Veja a [visão geral dos módulos](/pt/modules/) para saber o que cada um faz.

## Navegue pelo app

- **Seletor de módulos** (canto superior esquerdo): alterna entre os módulos instalados. Cada módulo tem toda a sua área de trabalho: barra lateral, páginas e conteúdo próprios.
- **Dashboard** (início): um quadro de blocos e widgets dos seus módulos, com quantas páginas você quiser. Veja [Dashboard e navegação](/pt/modules/dashboard).
- **Busca** (barra superior, <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, ou <kbd>/</kbd>): encontra módulos e seções de configurações; o botão de camadas a amplia para o seu próprio conteúdo.
- **Sino** (barra superior): a caixa de notificações, onde chegam lembretes e alertas de webhook.
- **Configurações**: conta, padrões, aparência, rede, módulos, dados, backup, atualizações, logs, connectors, cofre e mais. Veja a [referência de configurações](/pt/config/settings).

## Próximos passos recomendados

1. **Crie o hábito de fazer backup cedo**: veja [Backup e restauração](/pt/guide/backup-restore).
2. Se outros dispositivos devem acessar o app, leia [Rede e acesso remoto](/pt/config/network) antes de mudar qualquer coisa, e ative primeiro a [autenticação em dois fatores](/pt/config/security#totp).
3. Vai usar provedores externos de dados de mercado? Crie um **[data connector](/pt/config/connectors)** por conta em **Configurações → Data connectors** conforme precisar. O app funciona bem sem nenhum, e vários provedores dispensam chave.
