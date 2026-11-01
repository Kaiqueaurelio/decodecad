# DecodeCAD Civil — Plano de implementação

**Status:** planejamento técnico inicial  
**Branch:** `feature/decodecad-civil-foundation`  
**Objetivo:** evoluir o DecodeCAD com ferramentas reais de topografia e engenharia civil, sem substituir nem fragilizar o núcleo CAD existente.

## 1. Diretriz do produto

DecodeCAD Civil será um ambiente de engenharia civil integrado ao DecodeCAD. O CAD atual continuará responsável por desenho, documentos, comandos, visualização, edição e intercâmbio DXF/DWG. Os recursos civis serão módulos de domínio próprios, conectados ao motor por interfaces pequenas e testáveis.

A primeira entrega não tentará reproduzir todo o Civil 3D. Ela deverá resolver um fluxo de trabalho concreto e verificável: **importar pontos topográficos, conferir os dados, gerar uma superfície TIN, calcular curvas de nível e visualizar/editar o resultado no desenho**.

## 2. Diagnóstico da arquitetura atual

O workspace já possui crates separados para geometria (`geom`), documento (`doc`), renderização (`render`), leitura/escrita (`io`), motor de comandos (`engine`) e interface (`ui-egui`). A geometria usa coordenadas `f64` e já contempla vetores 2D/3D e primitivas; o documento ainda não possui uma entidade de superfície TIN ou um modelo de terreno dedicado.

Consequência: não criar um segundo motor CAD, não colocar regras de engenharia dentro da interface e não transformar cada ponto/triângulo em uma entidade DXF independente sem medir o impacto. O domínio civil deve ser separado do desenho genérico.

## 3. Arquitetura proposta

Criar crates novos, respeitando a ordem de dependências do workspace:

| Crate | Responsabilidade | Depende de |
|---|---|---|
| `civil-core` | unidades, coordenadas, metadados do projeto, erros e identificadores | `geom`, `serde` |
| `civil-survey` | pontos levantados, códigos, importação CSV e validação | `civil-core` |
| `civil-surface` | triangulação TIN, consultas de elevação, curvas de nível e volumes | `civil-core`, `geom` |
| `civil-io` | formatos de projeto civil e adaptadores de importação/exportação | crates civis + `io` apenas na camada de integração |

Integração existente:
- `engine`: comandos civis e operações de domínio; nenhuma regra de engenharia na UI.
- `render`: representação visual de pontos, arestas TIN e curvas; preferir primitivas/display lists antes de adicionar novos tipos persistidos ao documento.
- `ui-egui`: painéis de levantamento, propriedades da superfície e ferramentas de visualização.
- `io`: DXF para intercâmbio de geometria derivada, sem prometer que um DXF preserve todo o modelo paramétrico civil.

### Modelo de dados inicial

- `ProjectSettings`: unidade linear, unidade angular, sistema de coordenadas declarado, datum/projeção quando conhecido e precisão de exibição.
- `SurveyPoint`: ID estável, X, Y, Z, código, descrição e origem.
- `SurveyDataset`: conjunto de pontos, origem do arquivo, avisos e estatísticas de validação.
- `TinSurface`: vértices, triângulos, linhas de quebra e limite externo; manter referência aos pontos de origem.
- `ContourSet`: intervalo, elevação-base e polilinhas resultantes.
- `CivilProject`: configurações, levantamentos, superfícies e histórico de operações.

Não assumir que coordenadas X/Y são latitude/longitude. Armazenar o sistema de referência declarado pelo usuário; transformação entre sistemas será uma funcionalidade explícita, testada e documentada.

## 4. Primeira entrega funcional — Topografia MVP

### Fluxo que deverá funcionar de ponta a ponta

1. Criar/abrir um projeto civil.
2. Importar arquivo CSV de pontos com mapeamento de colunas (E, N, Z, código e descrição).
3. Validar linhas inválidas, campos ausentes, coordenadas não finitas, IDs duplicados e pontos coincidentes; apresentar relatório antes de aceitar a importação.
4. Exibir pontos no desenho e em uma tabela, com seleção sincronizada.
5. Gerar superfície TIN a partir dos pontos aceitos.
6. Permitir definir limite da superfície e linhas de quebra; não triangular através de limites ou descontinuidades.
7. Gerar curvas de nível por intervalo configurável.
8. Consultar elevação em uma posição e inspecionar triângulo/declividade.
9. Salvar, fechar e reabrir o projeto sem perder dados ou configurações.
10. Exportar desenho derivado para DXF e manter o projeto civil nativo como fonte de verdade.

### Critérios de aceite

- Um arquivo de exemplo documentado pode ser importado sem edição manual.
- O relatório informa quantos pontos foram importados, rejeitados e duplicados, com motivo por linha.
- A triangulação é determinística para a mesma entrada e não cria triângulos degenerados.
- Curvas de nível cruzam triângulos de forma consistente, não geram segmentos NaN/infinitos e respeitam o limite.
- Consulta de elevação retorna o resultado interpolado dentro de um triângulo e informa claramente quando o ponto está fora da superfície.
- Salvar/reabrir preserva pontos, TIN, curvas, unidades e referência espacial.
- Testes cobrem casos degenerados, conjunto vazio, três pontos colineares, pontos repetidos, bordas e valores extremos.
- A interface permanece responsiva em conjuntos de teste definidos; operações pesadas podem ser executadas fora do frame de UI.

## 5. Fases de desenvolvimento

### Fase 0 — Fundação e contratos
- Adicionar os crates e suas regras de dependência.
- Definir unidades e tipos de coordenadas sem conversões implícitas perigosas.
- Definir formato versionado do projeto civil e migração de versão.
- Criar fixtures pequenas e documentação de CSV.
- Registrar decisão sobre bibliotecas de triangulação após revisar licença, manutenção e compatibilidade WASM.

**Saída:** workspace compila; tipos e testes básicos prontos.

### Fase 1 — Levantamento
- Importador CSV configurável.
- Validação e relatório de erros.
- Tabela de pontos e seleção.
- Persistência do conjunto de levantamento.

**Saída:** importar, revisar, salvar e reabrir pontos.

### Fase 2 — Superfície TIN
- Triangulação 2.5D com algoritmo validado.
- Limite externo e linhas de quebra.
- Consultas de elevação e declividade.
- Renderização de pontos, arestas e faces.

**Saída:** superfície de terreno utilizável e inspecionável.

### Fase 3 — Curvas e análise
- Extração de curvas por intervalo.
- Cotas pontuais e rótulos básicos.
- Análise de declividade e mapa de cores opcional.
- Exportação DXF de curvas e pontos.

**Saída:** planta topográfica básica.

### Fase 4 — Terraplenagem
- Superfície existente e superfície projetada.
- Comparação de superfícies com método numérico documentado.
- Volumes de corte/aterro, relatório e mapa de diferenças.
- Testes com volumes analíticos conhecidos.

**Saída:** cálculo auditável de movimentação de terra.

### Fase 5 — Projeto geométrico viário
- Alinhamentos horizontais.
- Perfis longitudinais e verticais.
- Seções transversais.
- Corredores e cálculo de volumes por estaqueamento.

**Saída:** fluxo inicial de projeto de estrada.

### Fase 6 — Infraestrutura
- Redes de drenagem pluvial e estruturas.
- Bacias e áreas de contribuição.
- Loteamento e ferramentas de parcelamento.
- Intercâmbio com formatos civis abertos conforme necessidade e especificação.

**Saída:** módulos especializados, entregues individualmente e validados por profissionais.

## 6. Qualidade, segurança e interoperabilidade

- Preservar as regras do repositório: Rust, sem `unsafe`, sem panics em código de produção, erros como `Result`, validação de dados externos e limites de memória/recursão.
- Não introduzir dependência com licença incompatível. Revisar licença e manutenção de cada biblioteca antes de adotá-la.
- Não copiar código, dados, ícones, fontes ou materiais proprietários de Autodesk ou de outros CADs.
- Não descartar entidades DXF/DWG desconhecidas ao salvar. O modelo civil não deve piorar a fidelidade atual dos arquivos.
- Manter o núcleo civil independente de egui para permitir CLI, testes headless e builds suportados.
- Executar os gates do projeto (`cargo xtask ci`, incluindo fmt, clippy, testes, camadas e WASM) em cada etapa integrada.
- Validar resultados de engenharia com casos analíticos e revisão de profissional habilitado antes de apresentar cálculos como adequados para uso executivo.

## 7. Decisões ainda necessárias antes de codificar o motor

1. Formato de projeto civil: arquivo único versionado ou diretório de projeto. Proposta inicial: arquivo único versionado, com possibilidade de anexos.
2. Sistemas de coordenadas: primeiro armazenar e identificar; transformações ficam para uma etapa com biblioteca e testes próprios.
3. Algoritmo TIN: escolher entre implementação própria e biblioteca após revisão de licença, robustez numérica e WASM.
4. Formatos de entrada: CSV primeiro; LandXML e outros formatos entram após o fluxo MVP.
5. Escopo de plataforma: priorizar desktop; manter o núcleo compilável em WASM sempre que as dependências permitirem.

## 8. Próxima tarefa técnica

Começar pela **Fase 0**: inspecionar os tipos reais de geometria, renderização e persistência; criar `civil-core` e `civil-survey` mínimos; adicionar testes de unidades, coordenadas e validação de pontos; integrar os crates ao workspace sem alterar o comportamento do CAD atual.

A primeira entrega será considerada concluída somente quando o fluxo de importação, validação, persistência e reabertura de pontos funcionar no aplicativo, não apenas quando os módulos compilarem.
